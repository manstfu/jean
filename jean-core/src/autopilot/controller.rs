use serde_json::{json, Value};
use tauri::AppHandle;
use uuid::Uuid;

use crate::http_server::EmitExt;

use super::director;
use super::storage;
use super::types::{
    AnswerSource, AutopilotMission, CheckpointStatus, DirectorConfig, DirectorDecision,
    GrillCheckpoint, MissionEvent, MissionPhase, MissionStatus, MissionTaskStatus,
    RecordWorkerStoppedRequest, RespondAutopilotApprovalRequest, StartAutopilotMissionRequest,
    UpdateAutopilotPolicyRequest,
};
use super::worker;

const NATIVE_TERMINAL_SUBMIT_DELAY: std::time::Duration = std::time::Duration::from_millis(120);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Build the input sequence a real terminal paste would send to a native TUI.
///
/// Claude Code collapses multiline input into a `[Pasted text]` placeholder.
/// The text and the final Enter must arrive as separate input events; if Enter
/// is appended to the same PTY write, the TUI can process it before its paste
/// state has been committed and leave the prompt waiting in the editor.
fn native_terminal_prompt_input(prompt: &str) -> String {
    let prompt = prompt.trim_end_matches(['\r', '\n']);
    format!("\x1b[200~{prompt}\x1b[201~")
}

async fn send_native_terminal_prompt(terminal_id: &str, prompt: &str) -> Result<(), String> {
    crate::terminal::terminal_write(
        terminal_id.to_string(),
        native_terminal_prompt_input(prompt),
    )
    .await?;

    // Give the TUI one event-loop turn to commit the bracketed paste before
    // submitting it. This is intentionally short; it only affects the first
    // write of each bounded Autopilot task.
    tokio::time::sleep(NATIVE_TERMINAL_SUBMIT_DELAY).await;
    crate::terminal::terminal_write(terminal_id.to_string(), "\r".to_string()).await
}

fn validate_text(value: &str, field: &str, max_len: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} cannot be empty"));
    }
    if value.len() > max_len {
        return Err(format!("{field} is too long (max {max_len} characters)"));
    }
    Ok(())
}

fn validate_list(values: &[String], field: &str, max_items: usize) -> Result<(), String> {
    if values.len() > max_items {
        return Err(format!("{field} has too many entries (max {max_items})"));
    }
    for value in values {
        if value.len() > 2_000 {
            return Err(format!(
                "An entry in {field} is too long (max 2000 characters)"
            ));
        }
    }
    Ok(())
}

fn validate_start_request(request: &StartAutopilotMissionRequest) -> Result<(), String> {
    validate_text(&request.project_id, "projectId", 200)?;
    validate_text(&request.worktree_id, "worktreeId", 200)?;
    validate_text(&request.worktree_path, "worktreePath", 4_096)?;
    validate_text(&request.goal, "goal", 20_000)?;
    validate_list(&request.acceptance_criteria, "acceptanceCriteria", 100)?;
    validate_list(&request.constraints, "constraints", 100)?;
    validate_list(&request.non_goals, "nonGoals", 100)?;
    validate_text(&request.worker.backend, "worker.backend", 100)?;
    validate_text(&request.worker.surface, "worker.surface", 40)?;
    if !matches!(
        request.worker.surface.as_str(),
        "jean_chat" | "native_terminal"
    ) {
        return Err("worker.surface must be jean_chat or native_terminal".to_string());
    }
    validate_text(&request.worker.execution_mode, "worker.execution_mode", 40)?;
    if !matches!(
        request.worker.execution_mode.as_str(),
        "plan" | "build" | "yolo"
    ) {
        return Err("worker.execution_mode must be plan, build, or yolo".to_string());
    }
    validate_text(&request.director.backend, "director.backend", 100)?;
    validate_text(&request.director.model, "director.model", 200)?;
    if request.policy.max_iterations == 0 {
        return Err("policy.max_iterations must be greater than zero".to_string());
    }
    if request.policy.max_repeated_failures == 0 {
        return Err("policy.max_repeated_failures must be greater than zero".to_string());
    }
    Ok(())
}

fn validate_worker_backend(backend: &str) -> Result<(), String> {
    if matches!(
        backend,
        "claude" | "codex" | "opencode" | "cursor" | "pi" | "commandcode" | "grok" | "kimi"
    ) {
        Ok(())
    } else {
        Err(format!("Unsupported Autopilot Worker backend: {backend}"))
    }
}

fn validate_native_worker_backend(backend: &str) -> Result<(), String> {
    if matches!(backend, "claude" | "codex" | "opencode" | "kimi") {
        Ok(())
    } else {
        Err(format!(
            "Native Autopilot Worker is not supported for backend {backend} yet"
        ))
    }
}

async fn resolve_worker_model(
    app: &AppHandle,
    mission: &AutopilotMission,
) -> Result<String, String> {
    if let Some(model) = mission
        .worker
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        return Ok(model.to_string());
    }
    let preferences = crate::load_preferences(app.clone()).await?;
    let model = crate::autopilot_worker_model_for_backend(&preferences, &mission.worker.backend);
    validate_text(&model, "worker model", 200)?;
    Ok(model)
}

async fn ensure_worker_session(
    app: &AppHandle,
    mission: &AutopilotMission,
) -> Result<String, String> {
    if let Some(session_id) = mission
        .worker_session_id
        .as_deref()
        .map(str::trim)
        .filter(|session_id| !session_id.is_empty())
    {
        return Ok(session_id.to_string());
    }

    if mission.worker.surface == "native_terminal" {
        // Native Workers do not enqueue a Jean Chat message and therefore do
        // not need a persisted chat session. Keep a mission-scoped identity
        // so the existing Worker boundary and event model remain stable.
        return Ok(format!("autopilot-native-worker:{}", mission.id));
    }

    let sessions = crate::chat::get_sessions(
        app.clone(),
        mission.worktree_id.clone(),
        mission.worktree_path.clone(),
        None,
        Some(false),
    )
    .await?;
    if let Some(session_id) = sessions
        .active_session_id
        .or_else(|| sessions.sessions.first().map(|session| session.id.clone()))
    {
        return Ok(session_id);
    }

    let session = crate::chat::create_session(
        app.clone(),
        mission.worktree_id.clone(),
        mission.worktree_path.clone(),
        None,
        Some(mission.worker.backend.clone()),
        None,
        None,
        None,
        None,
        None,
    )
    .await?;
    Ok(session.id)
}

fn worker_prompt(mission: &AutopilotMission, task_id: &str) -> Result<String, String> {
    let task = mission
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| "Autopilot current task disappeared".to_string())?;
    let acceptance = if mission.acceptance_criteria.is_empty() {
        "No explicit acceptance criteria were supplied; infer only from the goal and repository evidence."
            .to_string()
    } else {
        mission
            .acceptance_criteria
            .iter()
            .map(|criterion| format!("- {criterion}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let constraints = mission
        .constraints
        .iter()
        .map(|constraint| format!("- {constraint}"))
        .collect::<Vec<_>>()
        .join("\n");
    let non_goals = mission
        .non_goals
        .iter()
        .map(|non_goal| format!("- {non_goal}"))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(format!(
        "[Jean Autopilot mission: {}]\n\n\
         You are the bounded Worker for one mission task. Work only in the selected worktree.\n\
         Mission goal:\n{}\n\n\
         Acceptance criteria:\n{}\n\n\
         Constraints:\n{}\n\n\
         Non-goals:\n{}\n\n\
         Current task: {}\n\
         Task instruction:\n{}\n\n\
         Before editing, perform the requested premortem or inspect the relevant evidence. \
         Make one bounded change set, run the smallest useful verification, and stop after \
         this task. Do not invent product decisions or widen scope. If a decision needs \
         human judgment, stop and explain the question. End with a concise summary of \
         changes, checks, failures, and remaining risks. End with these handoff lines so the \
         hidden Director can choose the next bounded task:\n\
         WORKER_STATUS: TASK_COMPLETE | MISSION_COMPLETE | BLOCKED | FAILED\n\
         SUGGESTED_NEXT_TASK: <one concise evidence-backed candidate, or NONE>\n",
        mission.id, mission.goal, acceptance, constraints, non_goals, task.title, task.instruction
    ))
}

#[derive(Debug, Default, PartialEq, Eq)]
struct WorkerHandoff {
    suggested_next_task: Option<String>,
    mission_complete: bool,
}

fn handoff_line_value(summary: &str, labels: &[&str]) -> Option<String> {
    summary.lines().find_map(|line| {
        let line = line.trim().trim_start_matches(['*', '#', '`', '>']).trim();
        let lower = line.to_ascii_lowercase();
        labels.iter().find_map(|label| {
            lower.strip_prefix(label).and_then(|_| {
                line.get(label.len()..)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
            })
        })
    })
}

fn parse_worker_handoff(summary: &str) -> WorkerHandoff {
    let suggested_value = handoff_line_value(
        summary,
        &["suggested_next_task:", "next_task:", "suggested next task:"],
    );
    let suggested_lower = suggested_value
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let suggested_is_none = suggested_lower == "none"
        || suggested_lower == "n/a"
        || suggested_lower.starts_with("none ")
        || suggested_lower.starts_with("none-")
        || suggested_lower.starts_with("none—");
    let suggested_next_task = if suggested_is_none {
        None
    } else {
        suggested_value.filter(|value| !matches!(value.to_ascii_lowercase().trim(), "none" | "n/a"))
    };
    let status = handoff_line_value(
        summary,
        &[
            "worker_status:",
            "task_status:",
            "mission_status:",
            "mission_complete:",
        ],
    )
    .unwrap_or_default()
    .to_ascii_lowercase();
    let status = status.trim();
    let explicit_mission_complete = status.starts_with("mission_complete")
        || status.starts_with("complete")
        || status == "true"
        || status == "yes";
    let task_is_done = status.starts_with("task_complete") || status.starts_with("done");
    let summary_reports_completion = summary.to_ascii_lowercase().contains("mission_complete");
    let mission_complete = explicit_mission_complete
        || (task_is_done && suggested_is_none && summary_reports_completion);

    WorkerHandoff {
        suggested_next_task,
        mission_complete,
    }
}

fn effective_worker_execution_mode(mission: &AutopilotMission) -> &str {
    // A persisted yolo selection is still subject to the mission policy. If
    // safe actions are not enabled, downgrade the actual Jean Chat request to
    // build mode so the backend cannot interpret the prompt as permission to
    // bypass all tool approvals.
    if mission.worker.execution_mode == "yolo" && !mission.policy.auto_approve_safe_actions {
        "build"
    } else {
        mission.worker.execution_mode.as_str()
    }
}

fn queued_worker_message(
    action_id: &str,
    prompt: String,
    mission: &AutopilotMission,
    model: &str,
) -> serde_json::Value {
    json!({
        "id": action_id,
        "message": prompt,
        "pendingImages": [],
        "pendingFiles": [],
        "pendingSkills": [],
        "pendingTextFiles": [],
        "model": model,
        "provider": mission.worker.provider,
        "executionMode": effective_worker_execution_mode(mission),
        "backend": mission.worker.backend,
        "allowAllTools": effective_worker_execution_mode(mission) == "yolo",
        "queuedAt": now_ms(),
    })
}

fn schedule_native_worker_result(
    app: AppHandle,
    request: RecordWorkerStoppedRequest,
    action_id: String,
) {
    crate::async_runtime::spawn(async move {
        if let Err(error) = record_autopilot_worker_stopped(app, request).await {
            log::warn!("Autopilot could not record native Worker action {action_id}: {error}");
        }
    });
}

fn emit_mission(app: &AppHandle, mission: &AutopilotMission) {
    if let Err(error) = app.emit_all(
        "autopilot:updated",
        &json!({ "mission": mission, "mission_id": mission.id }),
    ) {
        log::warn!("Failed to emit Autopilot update: {error}");
    }
}

fn append_and_emit(
    app: &AppHandle,
    mission: &AutopilotMission,
    event_type: &str,
    summary: &str,
    data: serde_json::Value,
) -> Result<AutopilotMission, String> {
    storage::append_event(
        app,
        &mission.id,
        &MissionEvent::new(event_type, summary, data),
    )?;
    emit_mission(app, mission);
    Ok(mission.clone())
}

pub async fn start_autopilot_mission(
    app: AppHandle,
    mut request: StartAutopilotMissionRequest,
) -> Result<AutopilotMission, String> {
    if request.director == DirectorConfig::default() {
        if let Ok(preferences) = crate::load_preferences(app.clone()).await {
            request.director = DirectorConfig {
                backend: preferences.autopilot_director_backend,
                model: preferences.autopilot_director_model,
                provider: preferences.autopilot_director_provider,
                effort: preferences.autopilot_director_effort,
            };
        }
    }
    validate_start_request(&request)?;
    validate_worker_backend(&request.worker.backend)?;
    let start_worker = request.start_worker;
    let mission = AutopilotMission::new(request);
    storage::create_mission(&app, &mission)?;
    let mission = append_and_emit(
        &app,
        &mission,
        "mission_created",
        "Autopilot mission created",
        json!({
            "phase": &mission.phase,
            "goal": &mission.goal,
            "director": &mission.director,
            "worker": &mission.worker,
        }),
    )?;
    if start_worker {
        launch_autopilot_worker(app, mission.id.clone()).await
    } else {
        Ok(mission)
    }
}

/// Start one bounded Worker task for the mission.
///
/// The floating robot always targets the session that opened it. Jean Chat
/// Workers use the persisted queue; native missions with `terminal_id` write
/// into that existing Jean-managed PTY. The one-shot native branch remains a
/// compatibility fallback for older missions without a terminal handle.
pub async fn launch_autopilot_worker(
    app: AppHandle,
    mission_id: String,
) -> Result<AutopilotMission, String> {
    let mission = storage::load_mission(&app, &mission_id)?;
    if mission.status != MissionStatus::Running {
        return Err("Autopilot mission is not running".to_string());
    }
    if mission.active_action_id.is_some() {
        return Err("Autopilot mission already has an active Worker action".to_string());
    }
    if mission.iteration >= mission.policy.max_iterations {
        return Err("Autopilot mission reached its iteration limit".to_string());
    }
    let task = mission
        .current_task_id
        .as_deref()
        .and_then(|task_id| mission.tasks.iter().find(|task| task.id == task_id))
        .ok_or_else(|| "Autopilot mission has no current task".to_string())?;
    if task.status == MissionTaskStatus::Running {
        return Err("Autopilot current task is already running".to_string());
    }
    if task.status == MissionTaskStatus::Completed {
        return Err("Autopilot current task is already completed".to_string());
    }

    validate_worker_backend(&mission.worker.backend)?;
    if mission.worker.surface == "native_terminal" {
        validate_native_worker_backend(&mission.worker.backend)?;
    }
    let session_id = ensure_worker_session(&app, &mission).await?;
    let model = resolve_worker_model(&app, &mission).await?;
    let action_id = Uuid::new_v4().to_string();
    let task_id = task.id.clone();
    let prompt = worker_prompt(&mission, &task_id)?;

    let prepared = storage::with_mission_mut(&app, &mission.id, |mission| {
        if mission.status != MissionStatus::Running {
            return Err("Autopilot mission stopped while preparing its Worker action".to_string());
        }
        if mission.active_action_id.is_some() {
            return Err("Autopilot mission already has an active Worker action".to_string());
        }
        let task = mission
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .ok_or_else(|| "Autopilot current task disappeared".to_string())?;
        task.status = MissionTaskStatus::Running;
        task.attempts = task.attempts.saturating_add(1);
        task.updated_at = now_ms();
        mission.worker_session_id = Some(session_id.clone());
        mission.active_action_id = Some(action_id.clone());
        mission.iteration = mission.iteration.saturating_add(1);
        Ok(mission.clone())
    })?;

    if prepared.worker.surface == "native_terminal" {
        let started = append_and_emit(
            &app,
            &prepared,
            "worker_task_started",
            "Autopilot native Worker started",
            json!({
                "action_id": &action_id,
                "task_id": &task_id,
                "worker_session_id": &session_id,
                "surface": &prepared.worker.surface,
                "backend": &prepared.worker.backend,
                "model": &model,
                "execution_mode": effective_worker_execution_mode(&prepared),
            }),
        )?;

        if let Some(terminal_id) = prepared.worker.terminal_id.as_deref() {
            let mission_id = prepared.id.clone();
            let prompt_result = send_native_terminal_prompt(terminal_id, &prompt).await;
            if let Err(error) = prompt_result {
                let failed = storage::with_mission_mut(&app, &mission_id, |mission| {
                    mission.status = MissionStatus::Failed;
                    mission.active_action_id = None;
                    if let Some(task) = mission.current_task_mut() {
                        task.status = MissionTaskStatus::Failed;
                        task.last_error = Some(error.clone());
                        task.updated_at = now_ms();
                    }
                    Ok(mission.clone())
                })?;
                return append_and_emit(
                    &app,
                    &failed,
                    "worker_start_failed",
                    "Autopilot could not write to the existing native terminal",
                    json!({ "error": error, "terminal_id": terminal_id }),
                );
            }

            let prompted = storage::load_mission(&app, &prepared.id)?;
            return append_and_emit(
                &app,
                &prompted,
                "worker_prompt_sent",
                "Autopilot task sent to the existing native terminal",
                json!({
                    "action_id": action_id,
                    "terminal_id": terminal_id,
                    "worker_session_id": session_id,
                }),
            );
        }

        let native_app = app.clone();
        let native_mission = prepared.clone();
        let native_prompt = prompt;
        let native_model = model;
        let native_session_id = session_id;
        let native_action_id = action_id;
        crate::async_runtime::spawn(async move {
            let mission_id = native_mission.id.clone();
            let worker_app = native_app.clone();
            let result = crate::async_runtime::spawn_blocking(move || {
                worker::run_native_worker_blocking(
                    &worker_app,
                    &native_mission,
                    &native_prompt,
                    &native_model,
                )
            })
            .await;
            let (succeeded, summary, evidence) = match result {
                Ok(Ok(result)) => (result.succeeded, result.summary, result.evidence),
                Ok(Err(error)) => (
                    false,
                    format!("Native Worker failed: {error}"),
                    vec![format!("native worker error: {error}")],
                ),
                Err(error) => (
                    false,
                    format!("Native Worker task failed to join: {error}"),
                    vec![format!("native worker join error: {error}")],
                ),
            };
            schedule_native_worker_result(
                native_app,
                RecordWorkerStoppedRequest {
                    mission_id,
                    worker_session_id: native_session_id,
                    succeeded,
                    summary,
                    evidence,
                },
                native_action_id,
            );
        });

        return Ok(started);
    }

    let queued = queued_worker_message(&action_id, prompt, &prepared, &model);
    let queue_result = async {
        crate::chat::set_session_model(
            app.clone(),
            prepared.worktree_id.clone(),
            prepared.worktree_path.clone(),
            session_id.clone(),
            model.clone(),
        )
        .await?;
        crate::chat::set_session_backend(
            app.clone(),
            prepared.worktree_id.clone(),
            prepared.worktree_path.clone(),
            session_id.clone(),
            prepared.worker.backend.clone(),
        )
        .await?;
        crate::chat::set_session_provider(
            app.clone(),
            prepared.worktree_id.clone(),
            prepared.worktree_path.clone(),
            session_id.clone(),
            prepared.worker.provider.clone(),
        )
        .await?;
        crate::chat::enqueue_message(
            app.clone(),
            prepared.worktree_id.clone(),
            prepared.worktree_path.clone(),
            session_id.clone(),
            queued,
        )
        .await?;
        Ok::<(), String>(())
    }
    .await;

    if let Err(error) = queue_result {
        let failed = storage::with_mission_mut(&app, &mission.id, |mission| {
            if mission.active_action_id.as_deref() == Some(action_id.as_str()) {
                mission.active_action_id = None;
                mission.status = MissionStatus::Failed;
                if let Some(task) = mission.tasks.iter_mut().find(|task| task.id == task_id) {
                    task.status = MissionTaskStatus::Failed;
                    task.last_error = Some(error.clone());
                    task.updated_at = now_ms();
                }
            }
            Ok(mission.clone())
        })?;
        append_and_emit(
            &app,
            &failed,
            "worker_start_failed",
            "Autopilot Worker could not be started",
            json!({ "error": &error, "action_id": action_id }),
        )?;
        return Err(error);
    }

    let started = storage::load_mission(&app, &mission.id)?;
    append_and_emit(
        &app,
        &started,
        "worker_task_started",
        "Autopilot Worker task queued",
        json!({
            "action_id": action_id,
            "task_id": task_id,
            "worker_session_id": session_id,
            "backend": &started.worker.backend,
            "model": model,
            "execution_mode": effective_worker_execution_mode(&started),
        }),
    )
}

pub async fn get_autopilot_mission(
    app: AppHandle,
    mission_id: String,
) -> Result<AutopilotMission, String> {
    storage::load_mission(&app, &mission_id)
}

pub async fn list_autopilot_missions(app: AppHandle) -> Result<Vec<AutopilotMission>, String> {
    storage::list_missions(&app)
}

pub async fn get_autopilot_mission_events(
    app: AppHandle,
    mission_id: String,
) -> Result<Vec<MissionEvent>, String> {
    storage::load_events(&app, &mission_id)
}

async fn cancel_worker_session(
    app: &AppHandle,
    mission: &AutopilotMission,
    worker_session_id: Option<&str>,
) {
    if mission.worker.surface == "native_terminal" {
        // Native one-shot adapters own their child process lifecycle. They do
        // not expose a Jean Chat cancellation handle, so do not send a
        // synthetic chat cancellation to the mission identity.
        log::debug!(
            "Native Autopilot Worker cancellation requested for mission {}",
            mission.id
        );
        return;
    }
    let Some(worker_session_id) = worker_session_id else {
        return;
    };
    match crate::chat::cancel_chat_message(
        app.clone(),
        worker_session_id.to_string(),
        mission.worktree_id.clone(),
    )
    .await
    {
        Ok(true) => log::info!(
            "Cancelled active Autopilot Worker session {} for mission {}",
            worker_session_id,
            mission.id
        ),
        Ok(false) => log::debug!(
            "Autopilot Worker session {} was already idle for mission {}",
            worker_session_id,
            mission.id
        ),
        Err(error) => log::warn!(
            "Failed to cancel Autopilot Worker session {} for mission {}: {}",
            worker_session_id,
            mission.id,
            error
        ),
    }
}

pub async fn pause_autopilot_mission(
    app: AppHandle,
    mission_id: String,
) -> Result<AutopilotMission, String> {
    let (mission, worker_session_id) = storage::with_mission_mut(&app, &mission_id, |mission| {
        if !matches!(
            &mission.status,
            MissionStatus::Running | MissionStatus::WaitingForHuman
        ) {
            return match &mission.status {
                MissionStatus::Paused => Err("Autopilot mission is already paused".to_string()),
                MissionStatus::Completed | MissionStatus::Failed | MissionStatus::Stopped => {
                    Err(format!(
                        "Cannot pause a {} Autopilot mission",
                        status_label(&mission.status)
                    ))
                }
                MissionStatus::Running | MissionStatus::WaitingForHuman => unreachable!(),
            };
        }

        let worker_session_id = mission
            .active_action_id
            .as_ref()
            .and(mission.worker_session_id.clone());
        mission.status = MissionStatus::Paused;
        mission.director_running = false;
        if mission.active_action_id.take().is_some() {
            if let Some(task) = mission.current_task_mut() {
                if task.status == MissionTaskStatus::Running {
                    task.status = MissionTaskStatus::Pending;
                    task.updated_at = now_ms();
                }
            }
        }
        Ok((mission.clone(), worker_session_id))
    })?;
    cancel_worker_session(&app, &mission, worker_session_id.as_deref()).await;
    append_and_emit(
        &app,
        &mission,
        "mission_paused",
        "Autopilot mission paused",
        json!({ "worker_cancel_requested": worker_session_id.is_some() }),
    )
}

pub async fn resume_autopilot_mission(
    app: AppHandle,
    mission_id: String,
) -> Result<AutopilotMission, String> {
    let mission = storage::with_mission_mut(&app, &mission_id, |mission| match &mission.status {
        MissionStatus::Paused | MissionStatus::WaitingForHuman => {
            let has_pending_checkpoint = mission
                .grill_checkpoints
                .iter()
                .any(|checkpoint| checkpoint.status == CheckpointStatus::Pending);
            mission.status = if has_pending_checkpoint {
                MissionStatus::WaitingForHuman
            } else {
                MissionStatus::Running
            };
            Ok(mission.clone())
        }
        MissionStatus::Running => Err("Autopilot mission is already running".to_string()),
        MissionStatus::Completed | MissionStatus::Failed | MissionStatus::Stopped => Err(format!(
            "Cannot resume a {} Autopilot mission",
            status_label(&mission.status)
        )),
    })?;
    let mission = append_and_emit(
        &app,
        &mission,
        "mission_resumed",
        "Autopilot mission resumed",
        json!({}),
    )?;
    if mission.status == MissionStatus::Running && mission.auto_start_worker {
        launch_autopilot_worker(app, mission.id.clone()).await
    } else {
        Ok(mission)
    }
}

pub async fn stop_autopilot_mission(
    app: AppHandle,
    mission_id: String,
) -> Result<AutopilotMission, String> {
    let (mission, worker_session_id) = storage::with_mission_mut(&app, &mission_id, |mission| {
        if mission.status.is_terminal() {
            return Err(format!(
                "Autopilot mission is already {}",
                status_label(&mission.status)
            ));
        }
        let worker_session_id = mission
            .active_action_id
            .as_ref()
            .and(mission.worker_session_id.clone());
        mission.status = MissionStatus::Stopped;
        mission.director_running = false;
        mission.active_action_id = None;
        if let Some(task) = mission.current_task_mut() {
            if task.status == MissionTaskStatus::Running {
                task.status = MissionTaskStatus::Blocked;
                task.last_error = Some("Mission stopped by user".to_string());
                task.updated_at = now_ms();
            }
        }
        Ok((mission.clone(), worker_session_id))
    })?;
    cancel_worker_session(&app, &mission, worker_session_id.as_deref()).await;
    append_and_emit(
        &app,
        &mission,
        "mission_stopped",
        "Autopilot mission stopped",
        json!({ "worker_cancel_requested": worker_session_id.is_some() }),
    )
}

pub async fn update_autopilot_policy(
    app: AppHandle,
    request: UpdateAutopilotPolicyRequest,
) -> Result<AutopilotMission, String> {
    if request.policy.max_iterations == 0 {
        return Err("policy.max_iterations must be greater than zero".to_string());
    }
    if request.policy.max_repeated_failures == 0 {
        return Err("policy.max_repeated_failures must be greater than zero".to_string());
    }
    let mission = storage::with_mission_mut(&app, &request.mission_id, |mission| {
        if mission.status.is_terminal() {
            return Err("Cannot update policy on a terminal Autopilot mission".to_string());
        }
        mission.policy = request.policy.clone();
        Ok(mission.clone())
    })?;
    append_and_emit(
        &app,
        &mission,
        "policy_updated",
        "Autopilot mission policy updated",
        json!({ "policy": &mission.policy }),
    )
}

/// Record a Worker boundary. This is the stable hand-off used by future Jean
/// Chat/native adapters: the adapter reports one bounded turn stopping, and
/// the controller creates a persisted grill checkpoint before any next task
/// can be scheduled.
pub async fn record_autopilot_worker_stopped(
    app: AppHandle,
    request: RecordWorkerStoppedRequest,
) -> Result<AutopilotMission, String> {
    validate_text(&request.summary, "summary", 20_000)?;
    let handoff = parse_worker_handoff(&request.summary);
    let mission = storage::with_mission_mut(&app, &request.mission_id, |mission| {
        if mission.status != MissionStatus::Running {
            return Err("Autopilot mission is not running".to_string());
        }
        if mission.worker_session_id.as_deref() != Some(request.worker_session_id.as_str()) {
            return Err("Worker session does not belong to this Autopilot mission".to_string());
        }
        if mission.current_task_id.as_deref().is_some_and(|task_id| {
            mission
                .tasks
                .iter()
                .find(|task| task.id == task_id)
                .is_some_and(|task| {
                    matches!(
                        task.status,
                        MissionTaskStatus::Completed | MissionTaskStatus::Failed
                    )
                })
        }) {
            return Err("Autopilot already recorded this Worker stop".to_string());
        }

        let task = mission
            .current_task_mut()
            .ok_or_else(|| "Autopilot mission has no current task".to_string())?;
        task.status = if request.succeeded {
            MissionTaskStatus::Completed
        } else {
            MissionTaskStatus::Failed
        };
        task.updated_at = now_ms();
        if !request.succeeded {
            task.last_error = Some(request.summary.clone());
        }
        mission.repeated_failure_count = if request.succeeded {
            0
        } else {
            mission.repeated_failure_count.saturating_add(1)
        };
        mission.last_worker_summary = Some(request.summary.clone());
        mission.last_worker_stopped_at = Some(now_ms());
        mission.active_action_id = None;
        mission.last_observation = Some(super::types::MissionObservation {
            worker_summary: Some(request.summary.clone()),
            suggested_next_task: handoff.suggested_next_task.clone(),
            mission_complete: handoff.mission_complete,
            evidence: request.evidence.clone(),
            ..Default::default()
        });
        mission.phase = MissionPhase::Grill;
        // A first failure is a normal mission transition: the hidden Director
        // should turn it into a bounded debug task. Escalate only after the
        // configured repeated-failure threshold, or when the Director itself
        // asks for a human decision.
        let requires_human = mission.repeated_failure_count >= mission.policy.max_repeated_failures;
        let checkpoint = GrillCheckpoint::pending(
            "What is the next evidence-backed step after this Worker stop?",
            if request.succeeded {
                "Inspect the changed files and run focused verification before assigning another implementation task."
            } else {
                "Treat the failure as a debug task, inspect the failure evidence, and rerun the smallest reproducer before changing scope."
            },
            "A Worker summary alone is not acceptance evidence; the next transition must be grounded in repository state and checks.",
            request.evidence.clone(),
            requires_human,
        );
        mission.grill_checkpoints.push(checkpoint);
        mission.status = MissionStatus::WaitingForHuman;
        Ok(mission.clone())
    })?;

    let mission = append_and_emit(
        &app,
        &mission,
        "worker_stopped",
        "Worker task stopped; grill checkpoint created",
        json!({
            "worker_session_id": request.worker_session_id,
            "succeeded": request.succeeded,
            "summary": request.summary,
            "evidence": request.evidence,
            "suggested_next_task": handoff.suggested_next_task,
            "mission_complete": handoff.mission_complete,
            "phase": &mission.phase,
        }),
    )?;

    if mission.auto_start_worker {
        let director_app = app.clone();
        let director_mission_id = mission.id.clone();
        crate::async_runtime::spawn(async move {
            if let Err(error) = run_autopilot_director(director_app, director_mission_id).await {
                log::warn!("Autopilot Director transition failed: {error}");
            }
        });
    }

    Ok(mission)
}

fn director_action_allows_auto_continue(
    policy: &super::types::MissionPolicy,
    decision: &DirectorDecision,
    checkpoint: &GrillCheckpoint,
) -> bool {
    if checkpoint.requires_human || decision.requires_human {
        return false;
    }
    match decision.action.as_str() {
        "approve_plan" => policy.auto_approve_plans,
        "approve_safe_action"
        | "send_worker_task"
        | "run_checks"
        | "run_review"
        | "create_debug_task"
        | "adopt_recommended_answer"
        | "mark_complete" => policy.auto_approve_safe_actions,
        _ => false,
    }
}

fn director_task_phase(decision: &DirectorDecision) -> MissionPhase {
    decision
        .phase
        .clone()
        .unwrap_or(match decision.action.as_str() {
            "create_debug_task" => MissionPhase::Fix,
            "run_checks" => MissionPhase::Verify,
            "run_review" => MissionPhase::Review,
            "approve_plan" => MissionPhase::Plan,
            _ => MissionPhase::Implement,
        })
}

fn director_task_title(decision: &DirectorDecision) -> &'static str {
    match decision.action.as_str() {
        "create_debug_task" => "Director debug task",
        "run_checks" => "Run Director verification",
        "run_review" => "Run Director review",
        "approve_plan" => "Implement approved Director plan",
        "adopt_recommended_answer" => "Continue from Director recommendation",
        _ => "Director Worker task",
    }
}

/// Run the hidden Director for a pending grill checkpoint and apply only the
/// policy-safe subset of its typed decision. Unsupported or failed Director
/// calls leave the checkpoint pending for a human instead of guessing.
pub async fn run_autopilot_director(
    app: AppHandle,
    mission_id: String,
) -> Result<AutopilotMission, String> {
    let mission = storage::with_mission_mut(&app, &mission_id, |mission| {
        if mission.status != MissionStatus::WaitingForHuman {
            return Err("Autopilot mission is not waiting for a Director checkpoint".to_string());
        }
        if mission.director_running {
            return Err("Autopilot Director is already running".to_string());
        }
        if !mission
            .grill_checkpoints
            .iter()
            .any(|checkpoint| checkpoint.status == CheckpointStatus::Pending)
        {
            return Err("Autopilot mission has no pending Director checkpoint".to_string());
        }
        mission.director_running = true;
        Ok(mission.clone())
    })?;

    let director_app = app.clone();
    let director_mission = mission.clone();
    let result = crate::async_runtime::spawn_blocking(move || {
        director::run_director_blocking(&director_app, &director_mission)
    })
    .await
    .map_err(|error| format!("Autopilot Director task failed to join: {error}"));

    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = storage::with_mission_mut(&app, &mission_id, |mission| {
                mission.director_running = false;
                Ok(mission.clone())
            });
            return Err(error);
        }
    };

    let decision = match result {
        Ok(decision) => decision,
        Err(error) => {
            if let Ok(current) = storage::with_mission_mut(&app, &mission_id, |mission| {
                mission.director_running = false;
                Ok(mission.clone())
            }) {
                let _ = append_and_emit(
                    &app,
                    &current,
                    "director_failed",
                    "Autopilot Director could not produce a decision",
                    json!({ "error": &error }),
                );
            }
            return Err(error);
        }
    };

    let transition = storage::with_mission_mut(&app, &mission_id, |mission| {
        mission.director_running = false;
        if mission.status != MissionStatus::WaitingForHuman {
            return Err(
                "Autopilot checkpoint was resolved while Director was thinking".to_string(),
            );
        }
        let checkpoint_id = mission
            .grill_checkpoints
            .iter()
            .find(|checkpoint| checkpoint.status == CheckpointStatus::Pending)
            .map(|checkpoint| checkpoint.id.clone())
            .ok_or_else(|| {
                "Autopilot checkpoint was resolved while Director was thinking".to_string()
            })?;

        let policy = mission.policy.clone();
        let worker_declared_mission_complete = mission
            .last_observation
            .as_ref()
            .is_some_and(|observation| observation.mission_complete);
        let (auto_continue, task_instruction, task_phase) = {
            let checkpoint = mission
                .pending_checkpoint_mut(&checkpoint_id)
                .ok_or_else(|| "Autopilot checkpoint disappeared".to_string())?;
            if let Some(question) = decision.question.clone() {
                checkpoint.question = question;
            }
            if let Some(recommended_answer) = decision.recommended_answer.clone() {
                checkpoint.recommended_answer = recommended_answer;
            }
            if let Some(why) = decision.why.clone() {
                checkpoint.why_it_matters = why;
            }
            if !decision.evidence.is_empty() {
                checkpoint.evidence = decision.evidence.clone();
            }
            checkpoint.recommended_task_instruction = decision.instruction.clone();
            if decision.requires_human
                || matches!(
                    decision.action.as_str(),
                    "ask_human_question" | "pause_for_human" | "stop"
                )
                || (decision.action == "mark_complete"
                    && (!worker_declared_mission_complete || !policy.auto_approve_safe_actions))
            {
                checkpoint.requires_human = true;
            }

            let auto_continue =
                director_action_allows_auto_continue(&policy, &decision, checkpoint)
                    && (decision.action != "mark_complete" || worker_declared_mission_complete);
            let task_instruction = decision
                .instruction
                .clone()
                .unwrap_or_else(|| checkpoint.recommended_answer.clone());
            let task_phase = director_task_phase(&decision);
            if auto_continue {
                checkpoint.status = CheckpointStatus::Adopted;
                checkpoint.adopted_answer = Some(task_instruction.clone());
                checkpoint.answer_source = Some(AnswerSource::Director);
                checkpoint.resolved_at = Some(now_ms());
            }
            (auto_continue, task_instruction, task_phase)
        };

        mission.decisions.push(decision.clone());
        let mission_completed = auto_continue && decision.action == "mark_complete";
        if mission_completed {
            mission.phase = MissionPhase::Complete;
            mission.status = MissionStatus::Completed;
            let current_task_id = mission.current_task_id.clone();
            if let Some(checkpoint) = mission.pending_checkpoint_mut(&checkpoint_id) {
                checkpoint.resulting_task_id = current_task_id;
            }
        } else if auto_continue {
            let previous_task_id = mission.current_task_id.clone();
            let mut next_task = super::types::MissionTask::new(
                director_task_title(&decision),
                task_instruction,
                task_phase.clone(),
            );
            if let Some(previous_task_id) = previous_task_id {
                next_task.dependencies.push(previous_task_id);
            }
            let next_task_id = next_task.id.clone();
            mission.tasks.push(next_task);
            mission.current_task_id = Some(next_task_id.clone());
            mission.phase = task_phase;
            mission.status = MissionStatus::Running;
            if let Some(checkpoint) = mission.pending_checkpoint_mut(&checkpoint_id) {
                checkpoint.resulting_task_id = Some(next_task_id);
            }
        } else {
            mission.status = MissionStatus::WaitingForHuman;
            mission.phase = MissionPhase::Grill;
        }
        Ok((
            mission.clone(),
            auto_continue,
            mission_completed,
            checkpoint_id,
        ))
    });

    let (updated, auto_continue, mission_completed, checkpoint_id) = match transition {
        Ok(transition) => transition,
        Err(error) => {
            let _ = storage::with_mission_mut(&app, &mission_id, |mission| {
                mission.director_running = false;
                Ok(mission.clone())
            });
            return Err(error);
        }
    };

    let updated = append_and_emit(
        &app,
        &updated,
        "director_decision",
        "Autopilot Director decision recorded",
        json!({
            "checkpoint_id": &checkpoint_id,
            "decision": &decision,
            "auto_continued": auto_continue,
            "mission_completed": mission_completed,
        }),
    )?;
    if let Err(error) = app.emit_all(
        "autopilot:decision",
        &json!({ "mission_id": &updated.id, "decision": &decision }),
    ) {
        log::debug!("Failed to emit Autopilot Director decision: {error}");
    }

    if auto_continue && !mission_completed && updated.auto_start_worker {
        launch_autopilot_worker(app, updated.id.clone()).await
    } else {
        Ok(updated)
    }
}

fn event_string(payload: &Value, camel: &str, snake: &str) -> Option<String> {
    payload
        .get(camel)
        .or_else(|| payload.get(snake))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

async fn collect_worker_summary(
    app: &AppHandle,
    mission: &AutopilotMission,
    fallback: String,
) -> (String, Vec<String>) {
    let mut evidence = vec![fallback.clone()];
    let Ok(session) = crate::chat::get_session(
        app.clone(),
        mission.worktree_id.clone(),
        mission.worktree_path.clone(),
        mission.worker_session_id.clone().unwrap_or_default(),
        Some(3),
    )
    .await
    else {
        return (fallback, evidence);
    };
    if let Some(message) = session
        .messages
        .iter()
        .rev()
        .find(|message| matches!(&message.role, crate::chat::types::MessageRole::Assistant))
    {
        if !message.content.trim().is_empty() {
            evidence.push("Latest persisted Worker assistant message was inspected.".to_string());
            return (message.content.clone(), evidence);
        }
    }
    (fallback, evidence)
}

async fn handle_worker_boundary_event(
    app: AppHandle,
    session_id: String,
    worktree_id: Option<String>,
    succeeded: bool,
    fallback_summary: String,
) {
    let missions = match storage::list_missions(&app) {
        Ok(missions) => missions,
        Err(error) => {
            log::warn!("Autopilot could not inspect missions after Worker event: {error}");
            return;
        }
    };
    for mission in missions {
        if mission.status != MissionStatus::Running
            || mission.active_action_id.is_none()
            || mission.worker_session_id.as_deref() != Some(session_id.as_str())
            || worktree_id
                .as_deref()
                .is_some_and(|worktree_id| worktree_id != mission.worktree_id)
        {
            continue;
        }
        let (summary, evidence) =
            collect_worker_summary(&app, &mission, fallback_summary.clone()).await;
        let request = RecordWorkerStoppedRequest {
            mission_id: mission.id.clone(),
            worker_session_id: session_id.clone(),
            succeeded,
            summary,
            evidence,
        };
        if let Err(error) = record_autopilot_worker_stopped(app.clone(), request).await {
            log::warn!(
                "Autopilot could not record Worker boundary for mission {}: {error}",
                mission.id
            );
        }
    }
}

/// Subscribe the mission controller to the shared chat lifecycle events.
///
/// The listener only observes sessions explicitly attached to a persisted
/// mission. It never treats an arbitrary visible terminal or chat session as
/// an Autopilot Worker.
pub fn install_event_handlers(app: &AppHandle) {
    let done_app = app.clone();
    app.listen("chat:done", move |event| {
        let payload: Value = match serde_json::from_str(event.payload()) {
            Ok(payload) => payload,
            Err(error) => {
                log::debug!("Ignoring malformed chat:done payload for Autopilot: {error}");
                return;
            }
        };
        if payload
            .get("waiting_for_plan")
            .or_else(|| payload.get("waitingForPlan"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return;
        }
        let Some(session_id) = event_string(&payload, "sessionId", "session_id") else {
            return;
        };
        let worktree_id = event_string(&payload, "worktreeId", "worktree_id");
        let app = done_app.clone();
        crate::async_runtime::spawn(async move {
            handle_worker_boundary_event(
                app,
                session_id,
                worktree_id,
                true,
                "chat:done reported a Worker turn boundary".to_string(),
            )
            .await;
        });
    });

    let error_app = app.clone();
    app.listen("chat:error", move |event| {
        let payload: Value = match serde_json::from_str(event.payload()) {
            Ok(payload) => payload,
            Err(error) => {
                log::debug!("Ignoring malformed chat:error payload for Autopilot: {error}");
                return;
            }
        };
        let Some(session_id) = event_string(&payload, "sessionId", "session_id") else {
            return;
        };
        let worktree_id = event_string(&payload, "worktreeId", "worktree_id");
        let summary = event_string(&payload, "error", "error")
            .unwrap_or_else(|| "Worker emitted chat:error".to_string());
        let app = error_app.clone();
        crate::async_runtime::spawn(async move {
            handle_worker_boundary_event(app, session_id, worktree_id, false, summary).await;
        });
    });

    let terminal_app = app.clone();
    app.listen("terminal:attention", move |event| {
        let payload: Value = match serde_json::from_str(event.payload()) {
            Ok(payload) => payload,
            Err(error) => {
                log::debug!("Ignoring malformed terminal:attention payload for Autopilot: {error}");
                return;
            }
        };
        let Some(session_id) = event_string(&payload, "sessionId", "session_id") else {
            return;
        };
        let succeeded = payload
            .get("succeeded")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let summary = event_string(&payload, "summary", "summary").unwrap_or_else(|| {
            "The native terminal reported that the Worker turn stopped".to_string()
        });
        let app = terminal_app.clone();
        crate::async_runtime::spawn(async move {
            handle_worker_boundary_event(app, session_id, None, succeeded, summary).await;
        });
    });

    let cancelled_app = app.clone();
    app.listen("chat:cancelled", move |event| {
        let payload: Value = match serde_json::from_str(event.payload()) {
            Ok(payload) => payload,
            Err(error) => {
                log::debug!("Ignoring malformed chat:cancelled payload for Autopilot: {error}");
                return;
            }
        };
        let Some(session_id) = event_string(&payload, "sessionId", "session_id") else {
            return;
        };
        let worktree_id = event_string(&payload, "worktreeId", "worktree_id");
        let app = cancelled_app.clone();
        crate::async_runtime::spawn(async move {
            handle_worker_boundary_event(
                app,
                session_id,
                worktree_id,
                false,
                "Worker turn was cancelled".to_string(),
            )
            .await;
        });
    });
}

pub async fn respond_autopilot_approval(
    app: AppHandle,
    request: RespondAutopilotApprovalRequest,
) -> Result<AutopilotMission, String> {
    let answer = request
        .answer
        .clone()
        .filter(|value| !value.trim().is_empty());
    let mission = storage::with_mission_mut(&app, &request.mission_id, |mission| {
        if mission.status != MissionStatus::WaitingForHuman {
            return Err("Autopilot mission is not waiting for a checkpoint answer".to_string());
        }
        if mission.director_running {
            return Err("Autopilot Director is still preparing the checkpoint".to_string());
        }

        let auto_approve_safe_actions = mission.policy.auto_approve_safe_actions;
        let (
            question,
            recommended_answer,
            why_it_matters,
            evidence,
            recommended_task_instruction,
            requires_human,
        ) = {
            let checkpoint = mission
                .pending_checkpoint_mut(&request.checkpoint_id)
                .ok_or_else(|| "Autopilot checkpoint not found".to_string())?;
            if checkpoint.status != CheckpointStatus::Pending {
                return Err("Autopilot checkpoint is already resolved".to_string());
            }
            if request.adopt_recommendation {
                if checkpoint.requires_human || !auto_approve_safe_actions {
                    return Err(
                        "This checkpoint requires an explicit human answer under the current policy"
                            .to_string(),
                    );
                }
                checkpoint.adopted_answer = Some(checkpoint.recommended_answer.clone());
                checkpoint.answer_source = Some(AnswerSource::Director);
                checkpoint.status = CheckpointStatus::Adopted;
            } else {
                let answer = answer.clone().ok_or_else(|| {
                    "A human answer is required when not adopting the recommendation".to_string()
                })?;
                checkpoint.human_answer = Some(answer);
                checkpoint.answer_source = Some(AnswerSource::Human);
                checkpoint.status = CheckpointStatus::Answered;
            }
            checkpoint.resolved_at = Some(now_ms());
            (
                checkpoint.question.clone(),
                checkpoint.recommended_answer.clone(),
                checkpoint.why_it_matters.clone(),
                checkpoint.evidence.clone(),
                checkpoint.recommended_task_instruction.clone(),
                checkpoint.requires_human,
            )
        };

        mission.status = MissionStatus::Running;
        mission.phase = if mission.repeated_failure_count > 0 {
            MissionPhase::Fix
        } else {
            MissionPhase::Verify
        };
        let previous_task_id = mission.current_task_id.clone();
        let (title, default_instruction) = if mission.repeated_failure_count > 0 {
            (
                "Debug the latest Worker failure",
                "Inspect the failure evidence, reproduce the smallest failing case, and make the smallest fix before rerunning focused verification.",
            )
        } else {
            (
                "Verify the latest Worker result",
                "Inspect the changed files, run focused checks, and compare the result against the mission acceptance criteria.",
            )
        };
        let instruction = if request.adopt_recommendation {
            recommended_task_instruction
                .filter(|instruction| !instruction.trim().is_empty())
                .unwrap_or_else(|| default_instruction.to_string())
        } else {
            answer
                .clone()
                .filter(|answer| !answer.trim().is_empty())
                .unwrap_or_else(|| default_instruction.to_string())
        };
        let mut next_task =
            super::types::MissionTask::new(title, instruction, mission.phase.clone());
        if let Some(previous_task_id) = previous_task_id {
            next_task.dependencies.push(previous_task_id);
        }
        let next_task_id = next_task.id.clone();
        mission.tasks.push(next_task);
        mission.current_task_id = Some(next_task_id.clone());
        if let Some(checkpoint) = mission.pending_checkpoint_mut(&request.checkpoint_id) {
            checkpoint.resulting_task_id = Some(next_task_id);
        }
        mission.decisions.push(DirectorDecision {
            id: uuid::Uuid::new_v4().to_string(),
            action: if request.adopt_recommendation {
                "adopt_recommended_answer".to_string()
            } else {
                "human_answer".to_string()
            },
            phase: Some(mission.phase.clone()),
            task_id: None,
            instruction: None,
            question: Some(question),
            recommended_answer: Some(recommended_answer),
            why: Some(why_it_matters),
            evidence,
            requires_human,
            raw: None,
            created_at: now_ms(),
        });
        Ok(mission.clone())
    })?;

    let mission = append_and_emit(
        &app,
        &mission,
        "checkpoint_answered",
        "Autopilot grill checkpoint resolved",
        json!({
            "checkpoint_id": request.checkpoint_id,
            "answer_source": if request.adopt_recommendation { "director" } else { "human" },
            "phase": &mission.phase,
        }),
    )?;
    if mission.auto_start_worker {
        launch_autopilot_worker(app, mission.id.clone()).await
    } else {
        Ok(mission)
    }
}

fn status_label(status: &MissionStatus) -> &'static str {
    match status {
        MissionStatus::Running => "running",
        MissionStatus::Paused => "paused",
        MissionStatus::WaitingForHuman => "waiting_for_human",
        MissionStatus::Completed => "completed",
        MissionStatus::Failed => "failed",
        MissionStatus::Stopped => "stopped",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autopilot::storage;
    use crate::autopilot::types::{DirectorConfig, MissionPolicy, WorkerConfig};

    fn request() -> StartAutopilotMissionRequest {
        StartAutopilotMissionRequest {
            project_id: "project".to_string(),
            worktree_id: "worktree".to_string(),
            worktree_path: "C:/worktree".to_string(),
            goal: "Ship the feature".to_string(),
            acceptance_criteria: vec!["Tests pass".to_string()],
            constraints: vec![],
            non_goals: vec![],
            worker: WorkerConfig {
                backend: "claude".to_string(),
                model: Some("haiku".to_string()),
                provider: None,
                execution_mode: "yolo".to_string(),
                surface: "jean_chat".to_string(),
                terminal_id: None,
            },
            director: DirectorConfig::default(),
            policy: MissionPolicy {
                auto_approve_safe_actions: true,
                ..MissionPolicy::default()
            },
            worker_session_id: Some("session".to_string()),
            start_worker: false,
        }
    }

    #[test]
    fn parses_worker_next_task_and_mission_completion_handoff() {
        let handoff = parse_worker_handoff(
            "Summary\nWORKER_STATUS: MISSION_COMPLETE\nSUGGESTED_NEXT_TASK: NONE",
        );
        assert_eq!(handoff.suggested_next_task, None);
        assert!(handoff.mission_complete);

        let handoff = parse_worker_handoff(
            "WORKER_STATUS: TASK_COMPLETE\nSUGGESTED_NEXT_TASK: Run focused verification",
        );
        assert_eq!(
            handoff.suggested_next_task.as_deref(),
            Some("Run focused verification")
        );
        assert!(!handoff.mission_complete);

        let handoff = parse_worker_handoff(
            "WORKER_STATUS: DONE\nSUGGESTED_NEXT_TASK: none - acceptance criteria satisfied; report MISSION_COMPLETE.",
        );
        assert_eq!(handoff.suggested_next_task, None);
        assert!(handoff.mission_complete);
    }

    #[test]
    fn worker_prompt_requires_a_director_handoff() {
        let mission = AutopilotMission::new(request());
        let prompt = worker_prompt(&mission, mission.current_task_id.as_deref().unwrap()).unwrap();
        assert!(prompt.contains("WORKER_STATUS:"));
        assert!(prompt.contains("SUGGESTED_NEXT_TASK:"));
    }

    #[tokio::test]
    async fn lifecycle_persists_and_emits_checkpoint() {
        let temp = tempfile::tempdir().unwrap();
        let app = AppHandle::new(temp.path().into(), temp.path().into()).unwrap();
        let mission = start_autopilot_mission(app.clone(), request())
            .await
            .unwrap();

        let paused = pause_autopilot_mission(app.clone(), mission.id.clone())
            .await
            .unwrap();
        assert_eq!(paused.status, MissionStatus::Paused);
        let resumed = resume_autopilot_mission(app.clone(), mission.id.clone())
            .await
            .unwrap();
        assert_eq!(resumed.status, MissionStatus::Running);

        let waiting = record_autopilot_worker_stopped(
            app.clone(),
            RecordWorkerStoppedRequest {
                mission_id: mission.id.clone(),
                worker_session_id: "session".to_string(),
                succeeded: true,
                summary: "Implemented the first slice".to_string(),
                evidence: vec!["focused test passed".to_string()],
            },
        )
        .await
        .unwrap();
        assert_eq!(waiting.status, MissionStatus::WaitingForHuman);
        assert_eq!(waiting.phase, MissionPhase::Grill);
        let checkpoint_id = waiting.grill_checkpoints[0].id.clone();
        storage::with_mission_mut(&app, &mission.id, |mission| {
            mission
                .pending_checkpoint_mut(&checkpoint_id)
                .unwrap()
                .recommended_task_instruction =
                Some("Run the focused verification now.".to_string());
            Ok(())
        })
        .unwrap();

        let continued = respond_autopilot_approval(
            app.clone(),
            RespondAutopilotApprovalRequest {
                mission_id: mission.id.clone(),
                checkpoint_id,
                answer: None,
                adopt_recommendation: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(continued.status, MissionStatus::Running);
        assert_eq!(continued.phase, MissionPhase::Verify);
        assert_eq!(continued.decisions.len(), 1);
        assert_eq!(
            continued.tasks.last().unwrap().instruction,
            "Run the focused verification now."
        );
        assert_eq!(storage::load_events(&app, &mission.id).unwrap().len(), 5);
    }

    #[tokio::test]
    async fn recommendation_cannot_bypass_human_policy() {
        let temp = tempfile::tempdir().unwrap();
        let app = AppHandle::new(temp.path().into(), temp.path().into()).unwrap();
        let mut request = request();
        request.policy.auto_approve_safe_actions = false;
        let mission = start_autopilot_mission(app.clone(), request).await.unwrap();
        let waiting = record_autopilot_worker_stopped(
            app.clone(),
            RecordWorkerStoppedRequest {
                mission_id: mission.id.clone(),
                worker_session_id: "session".to_string(),
                succeeded: true,
                summary: "Done".to_string(),
                evidence: vec![],
            },
        )
        .await
        .unwrap();
        let error = respond_autopilot_approval(
            app,
            RespondAutopilotApprovalRequest {
                mission_id: mission.id,
                checkpoint_id: waiting.grill_checkpoints[0].id.clone(),
                answer: None,
                adopt_recommendation: true,
            },
        )
        .await
        .unwrap_err();
        assert!(error.contains("explicit human answer"));
    }

    #[tokio::test]
    async fn pause_releases_active_worker_and_resume_preserves_checkpoint_gate() {
        let temp = tempfile::tempdir().unwrap();
        let app = AppHandle::new(temp.path().into(), temp.path().into()).unwrap();
        let mission = start_autopilot_mission(app.clone(), request())
            .await
            .unwrap();

        storage::with_mission_mut(&app, &mission.id, |mission| {
            mission.active_action_id = Some("action".to_string());
            mission.current_task_mut().unwrap().status = MissionTaskStatus::Running;
            Ok(())
        })
        .unwrap();

        let paused = pause_autopilot_mission(app.clone(), mission.id.clone())
            .await
            .unwrap();
        assert_eq!(paused.status, MissionStatus::Paused);
        assert_eq!(paused.active_action_id, None);
        assert_eq!(paused.tasks[0].status, MissionTaskStatus::Pending);

        let resumed = resume_autopilot_mission(app.clone(), mission.id.clone())
            .await
            .unwrap();
        assert_eq!(resumed.status, MissionStatus::Running);

        let waiting = record_autopilot_worker_stopped(
            app.clone(),
            RecordWorkerStoppedRequest {
                mission_id: mission.id.clone(),
                worker_session_id: "session".to_string(),
                succeeded: true,
                summary: "Done".to_string(),
                evidence: vec![],
            },
        )
        .await
        .unwrap();
        let paused_waiting = pause_autopilot_mission(app.clone(), mission.id.clone())
            .await
            .unwrap();
        assert_eq!(paused_waiting.status, MissionStatus::Paused);
        let resumed_waiting = resume_autopilot_mission(app, mission.id).await.unwrap();
        assert_eq!(resumed_waiting.status, MissionStatus::WaitingForHuman);
        assert_eq!(resumed_waiting.grill_checkpoints, waiting.grill_checkpoints);
    }

    #[test]
    fn status_label_is_stable_for_errors() {
        assert_eq!(
            status_label(&MissionStatus::WaitingForHuman),
            "waiting_for_human"
        );
    }

    #[test]
    fn director_safe_actions_follow_mission_policy() {
        let mut mission = AutopilotMission::new(request());
        mission.policy.auto_approve_safe_actions = false;
        let checkpoint = GrillCheckpoint::pending(
            "Continue?",
            "Run focused checks.",
            "Evidence is available.",
            vec![],
            false,
        );
        let decision = DirectorDecision {
            id: "decision".to_string(),
            action: "run_checks".to_string(),
            phase: Some(MissionPhase::Verify),
            task_id: None,
            instruction: Some("Run the focused checks.".to_string()),
            question: None,
            recommended_answer: None,
            why: None,
            evidence: vec![],
            requires_human: false,
            raw: None,
            created_at: 0,
        };

        assert!(!director_action_allows_auto_continue(
            &mission.policy,
            &decision,
            &checkpoint
        ));
        mission.policy.auto_approve_safe_actions = true;
        assert!(director_action_allows_auto_continue(
            &mission.policy,
            &decision,
            &checkpoint
        ));
    }

    #[test]
    fn worker_yolo_mode_is_downgraded_without_safe_action_policy() {
        let mut mission = AutopilotMission::new(request());
        mission.policy.auto_approve_safe_actions = false;

        let queued = queued_worker_message(
            "action",
            "Inspect the repository.".to_string(),
            &mission,
            "haiku",
        );

        assert_eq!(queued["executionMode"], "build");
        assert_eq!(queued["allowAllTools"], false);
    }

    #[test]
    fn worker_yolo_mode_requires_explicit_safe_action_policy() {
        let mission = AutopilotMission::new(request());
        let queued = queued_worker_message(
            "action",
            "Inspect the repository.".to_string(),
            &mission,
            "haiku",
        );

        assert_eq!(queued["executionMode"], "yolo");
        assert_eq!(queued["allowAllTools"], true);
    }

    #[test]
    fn native_worker_backend_allowlist_matches_structured_adapters() {
        for backend in ["claude", "codex", "opencode", "kimi"] {
            assert!(validate_native_worker_backend(backend).is_ok());
        }
        assert!(validate_native_worker_backend("cursor").is_err());
    }

    #[test]
    fn native_terminal_prompt_uses_bracketed_paste_without_trailing_newline() {
        assert_eq!(
            native_terminal_prompt_input("line one\nline two\n"),
            "\x1b[200~line one\nline two\x1b[201~"
        );
    }
}

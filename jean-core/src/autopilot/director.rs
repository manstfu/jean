use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;
use uuid::Uuid;

use super::types::{AutopilotMission, DirectorDecision, MissionPhase};

const MAX_DIRECTOR_OUTPUT_BYTES: usize = 64 * 1024;

/// Structured output schema shared by the supported headless Director
/// adapters. The Rust parser remains authoritative after backend extraction.
pub const DIRECTOR_RESPONSE_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "action": {
      "type": "string",
      "enum": [
        "send_worker_task",
        "approve_plan",
        "approve_safe_action",
        "run_checks",
        "run_review",
        "create_debug_task",
        "adopt_recommended_answer",
        "ask_human_question",
        "mark_complete",
        "pause_for_human",
        "stop"
      ]
    },
    "phase": {
      "type": "string",
      "enum": ["premortem", "plan", "implement", "verify", "grill", "review", "fix", "complete"]
    },
    "task_id": { "type": "string" },
    "instruction": { "type": "string" },
    "question": { "type": "string" },
    "recommended_answer": { "type": "string" },
    "why": { "type": "string" },
    "evidence": {
      "type": "array",
      "items": { "type": "string" },
      "maxItems": 100
    },
    "requires_human": { "type": "boolean" }
  },
  "required": ["action", "requires_human"],
  "additionalProperties": false
}"#;

/// Execute one bounded, tool-restricted Director call and normalize its JSON.
///
/// This is synchronous because the backend one-shot APIs own their process or
/// HTTP lifecycle. Callers running on Tauri's async runtime must use
/// `spawn_blocking` around this function. The Worker worktree is deliberately
/// not passed to the Director.
pub fn run_director_blocking(
    app: &AppHandle,
    mission: &AutopilotMission,
) -> Result<DirectorDecision, String> {
    let observation = build_director_observation(mission);
    let prompt = director_prompt_for_observation(&observation);
    let model = mission.director.model.trim();
    if model.is_empty() {
        return Err("Autopilot Director model cannot be empty".to_string());
    }
    let effort = mission.director.effort.as_deref();
    let raw = match mission
        .director
        .backend
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "claude" => crate::chat::claude::execute_one_shot_claude_read_only(
            app,
            &prompt,
            model,
            DIRECTOR_RESPONSE_SCHEMA,
        )?,
        "codex" => crate::chat::codex::execute_one_shot_codex_read_only(
            app,
            &prompt,
            model,
            DIRECTOR_RESPONSE_SCHEMA,
            effort,
        )?,
        "cursor" => crate::chat::cursor::execute_one_shot_cursor(app, &prompt, model, None)?,
        "grok" => crate::chat::grok::execute_one_shot_grok(
            app,
            &prompt,
            model,
            Some(DIRECTOR_RESPONSE_SCHEMA),
            None,
            effort,
        )?,
        "opencode" => crate::chat::opencode::execute_one_shot_opencode(
            app,
            &prompt,
            model,
            Some(DIRECTOR_RESPONSE_SCHEMA),
            None,
            effort,
        )?,
        backend => {
            return Err(format!(
                "Autopilot Director backend does not have a safe structured adapter yet: {backend}"
            ));
        }
    };

    parse_director_response(&raw)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DirectorAction {
    SendWorkerTask,
    ApprovePlan,
    ApproveSafeAction,
    RunChecks,
    RunReview,
    CreateDebugTask,
    AdoptRecommendedAnswer,
    AskHumanQuestion,
    MarkComplete,
    PauseForHuman,
    Stop,
}

impl DirectorAction {
    fn from_str(value: &str) -> Option<Self> {
        Some(match value {
            "send_worker_task" => Self::SendWorkerTask,
            "approve_plan" => Self::ApprovePlan,
            "approve_safe_action" => Self::ApproveSafeAction,
            "run_checks" => Self::RunChecks,
            "run_review" => Self::RunReview,
            "create_debug_task" => Self::CreateDebugTask,
            "adopt_recommended_answer" => Self::AdoptRecommendedAnswer,
            "ask_human_question" => Self::AskHumanQuestion,
            "mark_complete" => Self::MarkComplete,
            "pause_for_human" => Self::PauseForHuman,
            "stop" => Self::Stop,
            _ => return None,
        })
    }

    fn requires_worker_instruction(&self) -> bool {
        matches!(
            self,
            Self::SendWorkerTask | Self::CreateDebugTask | Self::RunChecks | Self::RunReview
        )
    }

    fn requires_question(&self) -> bool {
        matches!(self, Self::AskHumanQuestion | Self::PauseForHuman)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectorResponse {
    pub action: String,
    #[serde(default)]
    pub phase: Option<MissionPhase>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub recommended_answer: Option<String>,
    #[serde(default)]
    pub why: Option<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub requires_human: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

fn required_text(value: Option<&str>, field: &str, max_len: usize) -> Result<String, String> {
    let value = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Director response is missing {field}"))?;
    if value.len() > max_len {
        return Err(format!(
            "Director response field {field} is too long (max {max_len} characters)"
        ));
    }
    Ok(value.to_string())
}

fn optional_text(
    value: Option<&str>,
    field: &str,
    max_len: usize,
) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.len() > max_len {
        return Err(format!(
            "Director response field {field} is too long (max {max_len} characters)"
        ));
    }
    Ok(Some(value.to_string()))
}

/// Parse and validate the model-only Director contract.
///
/// This parser deliberately accepts only a JSON object and a bounded action
/// vocabulary. Backend-specific extraction (Claude structured tool calls,
/// Codex app-server output, etc.) belongs in the invocation adapter; once the
/// payload reaches this function, the controller gets one normalized decision.
pub fn parse_director_response(text: &str) -> Result<DirectorDecision, String> {
    if text.len() > MAX_DIRECTOR_OUTPUT_BYTES {
        return Err("Director response exceeds the 64 KiB limit".to_string());
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|error| format!("Director response is not valid JSON: {error}"))?;
    let response: DirectorResponse = serde_json::from_value(value.clone())
        .map_err(|error| format!("Director response has an invalid shape: {error}"))?;
    let action = DirectorAction::from_str(response.action.trim())
        .ok_or_else(|| format!("Unsupported Director action: {}", response.action))?;

    let instruction = if action.requires_worker_instruction() {
        Some(required_text(
            response.instruction.as_deref(),
            "instruction",
            20_000,
        )?)
    } else {
        optional_text(response.instruction.as_deref(), "instruction", 20_000)?
    };
    let question = if action.requires_question() {
        Some(required_text(
            response.question.as_deref(),
            "question",
            2_000,
        )?)
    } else {
        optional_text(response.question.as_deref(), "question", 2_000)?
    };
    let recommended_answer = optional_text(
        response.recommended_answer.as_deref(),
        "recommended_answer",
        4_000,
    )?;
    if response.requires_human && question.is_none() {
        return Err("A human-gated Director response must include one question".to_string());
    }
    if question.is_some() && recommended_answer.is_none() {
        return Err("A Director question must include a recommended_answer".to_string());
    }
    if response.evidence.len() > 100 {
        return Err("Director response contains too much evidence".to_string());
    }
    if response.evidence.iter().any(|item| item.len() > 2_000) {
        return Err("A Director evidence item is too long".to_string());
    }

    Ok(DirectorDecision {
        id: Uuid::new_v4().to_string(),
        action: response.action.trim().to_string(),
        phase: response.phase,
        task_id: optional_text(response.task_id.as_deref(), "task_id", 200)?,
        instruction,
        question,
        recommended_answer,
        why: optional_text(response.why.as_deref(), "why", 4_000)?,
        evidence: response.evidence,
        requires_human: response.requires_human,
        raw: Some(value),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    })
}

/// Build the bounded observation the hidden Director will receive.
///
/// This is data-only. It intentionally contains no terminal handles, shell
/// capabilities, or arbitrary filesystem access.
pub fn build_director_observation(mission: &AutopilotMission) -> Value {
    let current_task = mission
        .current_task_id
        .as_deref()
        .and_then(|task_id| mission.tasks.iter().find(|task| task.id == task_id));
    json!({
        "mission": {
            "id": mission.id,
            "goal": mission.goal,
            "acceptance_criteria": mission.acceptance_criteria,
            "constraints": mission.constraints,
            "non_goals": mission.non_goals,
            "status": mission.status,
            "phase": mission.phase,
            "iteration": mission.iteration,
            "max_iterations": mission.policy.max_iterations,
            "repeated_failure_count": mission.repeated_failure_count,
            "max_repeated_failures": mission.policy.max_repeated_failures,
        },
        "current_task": current_task,
        "worker": {
            "backend": mission.worker.backend,
            "model": mission.worker.model,
            "execution_mode": mission.worker.execution_mode,
            "session_id": mission.worker_session_id,
            "last_summary": mission.last_worker_summary,
            "suggested_next_task": mission
                .last_observation
                .as_ref()
                .and_then(|observation| observation.suggested_next_task.clone()),
            "mission_complete": mission
                .last_observation
                .as_ref()
                .map(|observation| observation.mission_complete)
                .unwrap_or(false),
        },
        "last_observation": mission.last_observation,
        "pending_checkpoints": mission
            .grill_checkpoints
            .iter()
            .filter(|checkpoint| {
                matches!(
                    checkpoint.status,
                    super::types::CheckpointStatus::Pending
                )
            })
            .collect::<Vec<_>>(),
        "recent_decisions": mission.decisions.iter().rev().take(10).collect::<Vec<_>>(),
    })
}

pub fn director_prompt_for_observation(observation: &Value) -> String {
    format!(
        "You are the hidden Director for a Jean Autopilot mission. \
         Inspect this bounded JSON observation and return exactly one JSON object \
         matching the Director contract. Ask at most one question. Include a \
         recommended_answer and why when a question is present. Never edit files, \
         run shell commands, use tools, or assume a product decision. Do not mark \
         the mission complete unless the Worker explicitly reports mission_complete \
         and the observation contains acceptance evidence. When the Worker provides \
         suggested_next_task, treat it as a candidate: improve it into the smallest \
         evidence-backed task, add the needed premortem or verification, and return \
         that refined instruction rather than blindly echoing it. A task stop means \
         the current task is over, not that the mission is over. \
         Observation:\n{}",
        serde_json::to_string(observation).unwrap_or_else(|_| "{}".to_string())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autopilot::types::{
        AutopilotMission, DirectorConfig, MissionPolicy, StartAutopilotMissionRequest, WorkerConfig,
    };

    fn mission() -> AutopilotMission {
        AutopilotMission::new(StartAutopilotMissionRequest {
            project_id: "project".to_string(),
            worktree_id: "worktree".to_string(),
            worktree_path: "C:/worktree".to_string(),
            goal: "Ship the feature".to_string(),
            acceptance_criteria: vec![],
            constraints: vec![],
            non_goals: vec![],
            worker: WorkerConfig::default(),
            director: DirectorConfig::default(),
            policy: MissionPolicy::default(),
            worker_session_id: None,
            start_worker: false,
        })
    }

    #[test]
    fn parses_one_typed_director_action() {
        let decision = parse_director_response(
            r#"{
                "action": "send_worker_task",
                "phase": "implement",
                "instruction": "Implement the focused fix.",
                "requires_human": false
            }"#,
        )
        .unwrap();
        assert_eq!(decision.action, "send_worker_task");
        assert_eq!(decision.phase, Some(MissionPhase::Implement));
        assert_eq!(
            decision.instruction.as_deref(),
            Some("Implement the focused fix.")
        );
    }

    #[test]
    fn rejects_questions_without_recommendations() {
        let error = parse_director_response(
            r#"{
                "action": "ask_human_question",
                "question": "Should we change scope?",
                "requires_human": true
            }"#,
        )
        .unwrap_err();
        assert!(error.contains("recommended_answer"));
    }

    #[test]
    fn rejects_unknown_actions() {
        let error = parse_director_response(
            r#"{
                "action": "run_everything",
                "instruction": "Do it."
            }"#,
        )
        .unwrap_err();
        assert!(error.contains("Unsupported Director action"));
    }

    #[test]
    fn rejects_oversized_output_before_parsing() {
        let error =
            parse_director_response(&"x".repeat(MAX_DIRECTOR_OUTPUT_BYTES + 1)).unwrap_err();
        assert!(error.contains("64 KiB"));
    }

    #[test]
    fn observation_is_data_only_and_bounded_to_recent_decisions() {
        let mut mission = mission();
        for index in 0..12 {
            mission.decisions.push(DirectorDecision {
                id: index.to_string(),
                action: "run_checks".to_string(),
                phase: Some(MissionPhase::Verify),
                task_id: None,
                instruction: None,
                question: None,
                recommended_answer: None,
                why: None,
                evidence: vec![],
                requires_human: false,
                raw: None,
                created_at: index,
            });
        }
        let observation = build_director_observation(&mission);
        assert_eq!(
            observation["recent_decisions"].as_array().map(Vec::len),
            Some(10)
        );
        assert!(observation.get("terminal").is_none());
        assert!(director_prompt_for_observation(&observation).contains("bounded JSON"));
    }

    #[test]
    fn response_schema_is_valid_and_requires_typed_action_fields() {
        let schema: Value = serde_json::from_str(DIRECTOR_RESPONSE_SCHEMA).unwrap();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"][0], "action");
        assert_eq!(schema["required"][1], "requires_human");
        assert!(schema["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action == "create_debug_task"));
    }
}

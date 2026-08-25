use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const AUTOPILOT_SCHEMA_VERSION: u32 = 1;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn default_schema_version() -> u32 {
    AUTOPILOT_SCHEMA_VERSION
}

fn default_director_backend() -> String {
    "codex".to_string()
}

fn default_director_model() -> String {
    // This is a preference fallback, not a claim about availability or price.
    // Settings/model-catalog resolution will replace it when the Director is
    // wired to a concrete backend.
    "gpt-5.4-mini".to_string()
}

fn default_worker_backend() -> String {
    "claude".to_string()
}

fn default_worker_execution_mode() -> String {
    "yolo".to_string()
}

fn default_worker_surface() -> String {
    "jean_chat".to_string()
}

fn default_max_iterations() -> u32 {
    50
}

fn default_max_repeated_failures() -> u32 {
    3
}

fn default_start_worker() -> bool {
    // The UI must explicitly opt into starting a coding Worker. This prevents
    // an older client that only knows how to create mission metadata from
    // unexpectedly launching an autonomous process.
    false
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Running,
    Paused,
    WaitingForHuman,
    Completed,
    Failed,
    Stopped,
}

impl Default for MissionStatus {
    fn default() -> Self {
        Self::Paused
    }
}

impl MissionStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Stopped)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionPhase {
    Premortem,
    Plan,
    Implement,
    Verify,
    Grill,
    Review,
    Fix,
    Complete,
}

impl Default for MissionPhase {
    fn default() -> Self {
        Self::Premortem
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionTaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Blocked,
}

impl Default for MissionTaskStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointStatus {
    Pending,
    Adopted,
    Answered,
    Paused,
}

impl Default for CheckpointStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnswerSource {
    Director,
    Human,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectorConfig {
    #[serde(default = "default_director_backend")]
    pub backend: String,
    #[serde(default = "default_director_model")]
    pub model: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
}

impl Default for DirectorConfig {
    fn default() -> Self {
        Self {
            backend: default_director_backend(),
            model: default_director_model(),
            provider: None,
            effort: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerConfig {
    #[serde(default = "default_worker_backend")]
    pub backend: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(alias = "executionMode", default = "default_worker_execution_mode")]
    pub execution_mode: String,
    /// `jean_chat` uses Jean's persisted chat queue. `native_terminal` uses
    /// the selected provider's own CLI protocol without turning it into a
    /// Jean Chat turn.
    #[serde(default = "default_worker_surface")]
    pub surface: String,
    /// Existing Jean-managed native terminal that receives Worker prompts.
    /// When absent, legacy missions use the one-shot compatibility adapter.
    #[serde(alias = "terminalId", default)]
    pub terminal_id: Option<String>,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            backend: default_worker_backend(),
            model: None,
            provider: None,
            execution_mode: default_worker_execution_mode(),
            surface: default_worker_surface(),
            terminal_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionPolicy {
    #[serde(alias = "autoApprovePlans", default)]
    pub auto_approve_plans: bool,
    #[serde(alias = "autoApproveSafeActions", default)]
    pub auto_approve_safe_actions: bool,
    #[serde(alias = "allowPush", default)]
    pub allow_push: bool,
    #[serde(alias = "allowMerge", default)]
    pub allow_merge: bool,
    #[serde(alias = "allowDelete", default)]
    pub allow_delete: bool,
    #[serde(alias = "maxIterations", default = "default_max_iterations")]
    pub max_iterations: u32,
    #[serde(
        alias = "maxRepeatedFailures",
        default = "default_max_repeated_failures"
    )]
    pub max_repeated_failures: u32,
}

impl Default for MissionPolicy {
    fn default() -> Self {
        Self {
            auto_approve_plans: false,
            auto_approve_safe_actions: false,
            allow_push: false,
            allow_merge: false,
            allow_delete: false,
            max_iterations: default_max_iterations(),
            max_repeated_failures: default_max_repeated_failures(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionTask {
    pub id: String,
    pub title: String,
    pub instruction: String,
    pub phase: MissionPhase,
    #[serde(default)]
    pub status: MissionTaskStatus,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl MissionTask {
    pub fn new(
        title: impl Into<String>,
        instruction: impl Into<String>,
        phase: MissionPhase,
    ) -> Self {
        let now = now_ms();
        Self {
            id: Uuid::new_v4().to_string(),
            title: title.into(),
            instruction: instruction.into(),
            phase,
            status: MissionTaskStatus::Pending,
            attempts: 0,
            dependencies: Vec::new(),
            last_error: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionObservation {
    #[serde(default)]
    pub worker_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_next_task: Option<String>,
    #[serde(default)]
    pub mission_complete: bool,
    #[serde(default)]
    pub git_status: Option<String>,
    #[serde(default)]
    pub changed_paths: Vec<String>,
    #[serde(default)]
    pub checks: Vec<String>,
    #[serde(default)]
    pub review_findings: Vec<String>,
    #[serde(default)]
    pub pending_approvals: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

impl Default for MissionObservation {
    fn default() -> Self {
        Self {
            worker_summary: None,
            suggested_next_task: None,
            mission_complete: false,
            git_status: None,
            changed_paths: Vec::new(),
            checks: Vec::new(),
            review_findings: Vec::new(),
            pending_approvals: Vec::new(),
            evidence: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectorDecision {
    pub id: String,
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
    pub raw: Option<Value>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrillCheckpoint {
    pub id: String,
    pub question: String,
    pub recommended_answer: String,
    pub why_it_matters: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub status: CheckpointStatus,
    #[serde(default)]
    pub requires_human: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adopted_answer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human_answer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_task_instruction: Option<String>,
    #[serde(default)]
    pub answer_source: Option<AnswerSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resulting_task_id: Option<String>,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<u64>,
}

impl GrillCheckpoint {
    pub fn pending(
        question: impl Into<String>,
        recommended_answer: impl Into<String>,
        why_it_matters: impl Into<String>,
        evidence: Vec<String>,
        requires_human: bool,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            question: question.into(),
            recommended_answer: recommended_answer.into(),
            why_it_matters: why_it_matters.into(),
            evidence,
            status: CheckpointStatus::Pending,
            requires_human,
            adopted_answer: None,
            human_answer: None,
            recommended_task_instruction: None,
            answer_source: None,
            resulting_task_id: None,
            created_at: now_ms(),
            resolved_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionEvent {
    pub id: String,
    pub event_type: String,
    pub summary: String,
    #[serde(default)]
    pub data: Value,
    pub created_at: u64,
}

impl MissionEvent {
    pub fn new(event_type: impl Into<String>, summary: impl Into<String>, data: Value) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            event_type: event_type.into(),
            summary: summary.into(),
            data,
            created_at: now_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutopilotMission {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub project_id: String,
    pub worktree_id: String,
    pub worktree_path: String,
    pub goal: String,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub non_goals: Vec<String>,
    #[serde(default)]
    pub status: MissionStatus,
    #[serde(default)]
    pub phase: MissionPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_session_id: Option<String>,
    #[serde(default)]
    pub worker: WorkerConfig,
    #[serde(default)]
    pub director: DirectorConfig,
    #[serde(default)]
    pub policy: MissionPolicy,
    #[serde(default = "default_start_worker")]
    pub auto_start_worker: bool,
    #[serde(default)]
    pub director_running: bool,
    #[serde(default)]
    pub tasks: Vec<MissionTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_task_id: Option<String>,
    #[serde(default)]
    pub grill_checkpoints: Vec<GrillCheckpoint>,
    #[serde(default)]
    pub decisions: Vec<DirectorDecision>,
    #[serde(default)]
    pub iteration: u32,
    #[serde(default)]
    pub repeated_failure_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observation: Option<MissionObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_action_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_worker_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_worker_stopped_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl AutopilotMission {
    pub fn new(request: StartAutopilotMissionRequest) -> Self {
        let now = now_ms();
        let premortem = MissionTask::new(
            "Run premortem",
            "Identify likely failure modes, the evidence that would detect them, and mitigations before implementation begins.",
            MissionPhase::Premortem,
        );
        let premortem_id = premortem.id.clone();

        Self {
            schema_version: AUTOPILOT_SCHEMA_VERSION,
            id: Uuid::new_v4().to_string(),
            project_id: request.project_id,
            worktree_id: request.worktree_id,
            worktree_path: request.worktree_path,
            goal: request.goal,
            acceptance_criteria: request.acceptance_criteria,
            constraints: request.constraints,
            non_goals: request.non_goals,
            status: MissionStatus::Running,
            phase: MissionPhase::Premortem,
            worker_session_id: request.worker_session_id,
            worker: request.worker,
            director: request.director,
            policy: request.policy,
            auto_start_worker: request.start_worker,
            director_running: false,
            tasks: vec![premortem],
            current_task_id: Some(premortem_id),
            grill_checkpoints: Vec::new(),
            decisions: Vec::new(),
            iteration: 0,
            repeated_failure_count: 0,
            last_failure_fingerprint: None,
            last_observation: None,
            active_action_id: None,
            last_worker_summary: None,
            last_worker_stopped_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn current_task_mut(&mut self) -> Option<&mut MissionTask> {
        let task_id = self.current_task_id.as_deref()?;
        self.tasks.iter_mut().find(|task| task.id == task_id)
    }

    pub fn pending_checkpoint_mut(&mut self, checkpoint_id: &str) -> Option<&mut GrillCheckpoint> {
        self.grill_checkpoints
            .iter_mut()
            .find(|checkpoint| checkpoint.id == checkpoint_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StartAutopilotMissionRequest {
    #[serde(rename = "projectId", alias = "project_id")]
    pub project_id: String,
    #[serde(rename = "worktreeId", alias = "worktree_id")]
    pub worktree_id: String,
    #[serde(rename = "worktreePath", alias = "worktree_path")]
    pub worktree_path: String,
    pub goal: String,
    #[serde(rename = "acceptanceCriteria", alias = "acceptance_criteria", default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(rename = "nonGoals", alias = "non_goals", default)]
    pub non_goals: Vec<String>,
    #[serde(default)]
    pub worker: WorkerConfig,
    #[serde(default)]
    pub director: DirectorConfig,
    #[serde(default)]
    pub policy: MissionPolicy,
    #[serde(rename = "workerSessionId", alias = "worker_session_id", default)]
    pub worker_session_id: Option<String>,
    #[serde(
        rename = "startWorker",
        alias = "start_worker",
        default = "default_start_worker"
    )]
    pub start_worker: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StartAutopilotWorkerRequest {
    #[serde(rename = "missionId", alias = "mission_id")]
    pub mission_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateAutopilotPolicyRequest {
    #[serde(rename = "missionId", alias = "mission_id")]
    pub mission_id: String,
    pub policy: MissionPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordWorkerStoppedRequest {
    #[serde(rename = "missionId", alias = "mission_id")]
    pub mission_id: String,
    #[serde(rename = "workerSessionId", alias = "worker_session_id")]
    pub worker_session_id: String,
    #[serde(default)]
    pub succeeded: bool,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RespondAutopilotApprovalRequest {
    #[serde(rename = "missionId", alias = "mission_id")]
    pub mission_id: String,
    #[serde(rename = "checkpointId", alias = "checkpoint_id")]
    pub checkpoint_id: String,
    #[serde(default)]
    pub answer: Option<String>,
    #[serde(
        rename = "adoptRecommendation",
        alias = "adopt_recommendation",
        default
    )]
    pub adopt_recommendation: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mission() -> AutopilotMission {
        AutopilotMission::new(StartAutopilotMissionRequest {
            project_id: "project".to_string(),
            worktree_id: "worktree".to_string(),
            worktree_path: "C:/worktree".to_string(),
            goal: "Ship the feature".to_string(),
            acceptance_criteria: vec!["Tests pass".to_string()],
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
    fn new_mission_starts_with_premortem_task() {
        let mission = mission();
        assert_eq!(mission.status, MissionStatus::Running);
        assert_eq!(mission.phase, MissionPhase::Premortem);
        assert_eq!(mission.tasks.len(), 1);
        assert_eq!(mission.tasks[0].phase, MissionPhase::Premortem);
        assert_eq!(mission.current_task_id, Some(mission.tasks[0].id.clone()));
    }

    #[test]
    fn persisted_fields_use_snake_case() {
        let json = serde_json::to_value(mission()).unwrap();
        assert!(json.get("worktree_id").is_some());
        assert!(json.get("current_task_id").is_some());
        assert!(json.get("worktreeId").is_none());
    }

    #[test]
    fn checkpoint_keeps_recommendation_separate_from_human_answer() {
        let checkpoint = GrillCheckpoint::pending(
            "Should we change the API?",
            "Keep the API and add a regression test.",
            "This avoids widening scope without evidence.",
            vec!["Existing tests cover the current API".to_string()],
            false,
        );

        assert_eq!(checkpoint.status, CheckpointStatus::Pending);
        assert_eq!(checkpoint.adopted_answer, None);
        assert_eq!(checkpoint.human_answer, None);
        assert!(!checkpoint.requires_human);
    }
}

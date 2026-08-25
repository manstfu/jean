import type { CliBackend } from './preferences'
import type { EffortLevel, ExecutionMode } from './chat'

export type AutopilotMissionStatus =
  | 'running'
  | 'paused'
  | 'waiting_for_human'
  | 'completed'
  | 'failed'
  | 'stopped'

export type AutopilotMissionPhase =
  | 'premortem'
  | 'plan'
  | 'implement'
  | 'verify'
  | 'grill'
  | 'review'
  | 'fix'
  | 'complete'

export type AutopilotTaskStatus =
  | 'pending'
  | 'running'
  | 'completed'
  | 'failed'
  | 'blocked'

export type AutopilotCheckpointStatus =
  | 'pending'
  | 'adopted'
  | 'answered'
  | 'paused'

export type AutopilotAnswerSource = 'director' | 'human'

export interface AutopilotDirectorConfig {
  backend: CliBackend | string
  model: string
  provider: string | null
  effort: EffortLevel | string | null
}

export interface AutopilotWorkerConfig {
  backend: CliBackend | string
  model: string | null
  provider: string | null
  execution_mode: ExecutionMode
  /** Jean Chat queue or the selected provider's native CLI protocol. */
  surface: 'jean_chat' | 'native_terminal'
  /** Existing native terminal to receive the next bounded task. */
  terminal_id?: string | null
}

export interface AutopilotPolicy {
  auto_approve_plans: boolean
  auto_approve_safe_actions: boolean
  allow_push: boolean
  allow_merge: boolean
  allow_delete: boolean
  max_iterations: number
  max_repeated_failures: number
}

export interface AutopilotTask {
  id: string
  title: string
  instruction: string
  phase: AutopilotMissionPhase
  status: AutopilotTaskStatus
  attempts: number
  dependencies: string[]
  last_error?: string | null
  created_at: number
  updated_at: number
}

export interface AutopilotObservation {
  worker_summary?: string | null
  suggested_next_task?: string | null
  mission_complete: boolean
  git_status?: string | null
  changed_paths: string[]
  checks: string[]
  review_findings: string[]
  pending_approvals: string[]
  evidence: string[]
}

export interface AutopilotDecision {
  id: string
  action: string
  phase?: AutopilotMissionPhase | null
  task_id?: string | null
  instruction?: string | null
  question?: string | null
  recommended_answer?: string | null
  why?: string | null
  evidence: string[]
  requires_human: boolean
  raw?: unknown
  created_at: number
}

export interface AutopilotCheckpoint {
  id: string
  question: string
  recommended_answer: string
  why_it_matters: string
  evidence: string[]
  status: AutopilotCheckpointStatus
  requires_human: boolean
  adopted_answer?: string | null
  human_answer?: string | null
  recommended_task_instruction?: string | null
  answer_source?: AutopilotAnswerSource | null
  resulting_task_id?: string | null
  created_at: number
  resolved_at?: number | null
}

export interface AutopilotMission {
  schema_version: number
  id: string
  project_id: string
  worktree_id: string
  worktree_path: string
  goal: string
  acceptance_criteria: string[]
  constraints: string[]
  non_goals: string[]
  status: AutopilotMissionStatus
  phase: AutopilotMissionPhase
  worker_session_id?: string | null
  worker: AutopilotWorkerConfig
  director: AutopilotDirectorConfig
  policy: AutopilotPolicy
  auto_start_worker: boolean
  director_running: boolean
  tasks: AutopilotTask[]
  current_task_id?: string | null
  grill_checkpoints: AutopilotCheckpoint[]
  decisions: AutopilotDecision[]
  iteration: number
  repeated_failure_count: number
  last_failure_fingerprint?: string | null
  last_observation?: AutopilotObservation | null
  active_action_id?: string | null
  last_worker_summary?: string | null
  last_worker_stopped_at?: number | null
  created_at: number
  updated_at: number
}

export interface AutopilotMissionEvent {
  id: string
  event_type: string
  summary: string
  data: unknown
  created_at: number
}

export interface StartAutopilotMissionInput {
  [key: string]: unknown
  projectId: string
  worktreeId: string
  worktreePath: string
  goal: string
  acceptanceCriteria?: string[]
  constraints?: string[]
  nonGoals?: string[]
  worker?: Partial<AutopilotWorkerConfig>
  director?: Partial<AutopilotDirectorConfig>
  policy?: Partial<AutopilotPolicy>
  workerSessionId?: string | null
  startWorker?: boolean
}

export interface AutopilotApprovalInput {
  [key: string]: unknown
  missionId: string
  checkpointId: string
  answer?: string | null
  adoptRecommendation: boolean
}

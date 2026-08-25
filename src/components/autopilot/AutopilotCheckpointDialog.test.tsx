import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@/test/test-utils'
import type { AutopilotMission } from '@/types/autopilot'
import { AutopilotCheckpointDialog } from './AutopilotCheckpointDialog'

let missions: AutopilotMission[]
const respondMutate = vi.fn()

vi.mock('@/services/autopilot', () => ({
  useAutopilotMissions: () => ({ data: missions }),
  useAutopilotUpdates: () => undefined,
  useRespondAutopilotApproval: () => ({
    mutate: respondMutate,
    isPending: false,
  }),
}))

function waitingMission(): AutopilotMission {
  return {
    schema_version: 1,
    id: 'mission-1',
    project_id: 'project-1',
    worktree_id: 'worktree-1',
    worktree_path: '/tmp/worktree-1',
    goal: 'Finish the orchestrator',
    acceptance_criteria: [],
    constraints: [],
    non_goals: [],
    status: 'waiting_for_human',
    phase: 'grill',
    worker_session_id: 'session-1',
    worker: {
      backend: 'claude',
      model: null,
      provider: null,
      execution_mode: 'yolo',
      surface: 'jean_chat',
    },
    director: {
      backend: 'codex',
      model: 'gpt-5.4-mini',
      provider: null,
      effort: 'low',
    },
    policy: {
      auto_approve_plans: true,
      auto_approve_safe_actions: true,
      allow_push: false,
      allow_merge: false,
      allow_delete: false,
      max_iterations: 50,
      max_repeated_failures: 3,
    },
    auto_start_worker: true,
    director_running: false,
    tasks: [],
    current_task_id: null,
    grill_checkpoints: [
      {
        id: 'checkpoint-1',
        question: 'What should happen next?',
        recommended_answer: 'Run focused verification',
        why_it_matters: 'A summary is not acceptance evidence.',
        evidence: ['The Worker changed src/autopilot.'],
        status: 'pending',
        requires_human: true,
        adopted_answer: null,
        human_answer: null,
        recommended_task_instruction: 'Run the focused checks.',
        answer_source: null,
        resulting_task_id: null,
        created_at: 1,
        resolved_at: null,
      },
    ],
    decisions: [],
    iteration: 1,
    repeated_failure_count: 0,
    last_failure_fingerprint: null,
    last_observation: null,
    active_action_id: null,
    last_worker_summary: 'The Worker stopped after its bounded task.',
    last_worker_stopped_at: 1,
    created_at: 1,
    updated_at: 2,
  }
}

describe('AutopilotCheckpointDialog', () => {
  beforeEach(() => {
    missions = []
    respondMutate.mockReset()
  })

  it('shows one Director checkpoint and submits the recommendation as a human answer', () => {
    missions = [waitingMission()]
    respondMutate.mockImplementation(
      (_input: unknown, options?: { onSuccess?: () => void }) =>
        options?.onSuccess?.()
    )

    render(<AutopilotCheckpointDialog />)

    expect(screen.getByText('Autopilot needs one decision')).toBeInTheDocument()
    expect(screen.getAllByText('Run focused verification')).not.toHaveLength(0)
    expect(screen.getByLabelText('Your answer')).toHaveValue(
      'Run the focused checks.'
    )
    expect(
      screen.getByText('The Worker changed src/autopilot.')
    ).toBeInTheDocument()

    fireEvent.click(
      screen.getByRole('button', { name: 'Accept recommendation' })
    )

    expect(respondMutate).toHaveBeenCalledWith(
      {
        missionId: 'mission-1',
        checkpointId: 'checkpoint-1',
        answer: 'Run the focused checks.',
        adoptRecommendation: false,
      },
      expect.any(Object)
    )
  })

  it('does not expose the generic checkpoint while the Director is running', () => {
    const mission = waitingMission()
    mission.director_running = true
    missions = [mission]

    render(<AutopilotCheckpointDialog />)

    expect(
      screen.queryByText('Autopilot needs one decision')
    ).not.toBeInTheDocument()
  })
})

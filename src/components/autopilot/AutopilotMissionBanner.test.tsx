import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@/test/test-utils'
import type { AutopilotMission } from '@/types/autopilot'
import { AutopilotMissionBanner } from './AutopilotMissionBanner'

let missions: AutopilotMission[] = []
const startMutate = vi.fn()
const pauseMutate = vi.fn()
const resumeMutate = vi.fn()
const stopMutate = vi.fn()

vi.mock('@/services/autopilot', () => ({
  useAutopilotMissions: () => ({ data: missions }),
  useStartAutopilotMission: () => ({
    mutate: startMutate,
    isPending: false,
  }),
  usePauseAutopilotMission: () => ({
    mutate: pauseMutate,
    isPending: false,
  }),
  useResumeAutopilotMission: () => ({
    mutate: resumeMutate,
    isPending: false,
  }),
  useStopAutopilotMission: () => ({
    mutate: stopMutate,
    isPending: false,
  }),
}))

function props() {
  return {
    projectId: 'project-1',
    worktreeId: 'worktree-1',
    worktreePath: '/tmp/worktree-1',
    sessionId: 'session-1',
    workerBackend: 'claude' as const,
    workerModel: 'claude-sonnet-5',
    workerProvider: null,
    primarySurface: 'chat' as const,
  }
}

function runningMission(sessionId = 'session-1'): AutopilotMission {
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
    status: 'running',
    phase: 'implement',
    worker_session_id: sessionId,
    worker: {
      backend: 'claude',
      model: 'claude-sonnet-5',
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
    tasks: [
      {
        id: 'task-1',
        title: 'Implement the next bounded task',
        instruction:
          'Inspect the current evidence and implement the next task.',
        phase: 'implement',
        status: 'running',
        attempts: 1,
        dependencies: [],
        last_error: null,
        created_at: 1,
        updated_at: 2,
      },
    ],
    current_task_id: 'task-1',
    grill_checkpoints: [],
    decisions: [],
    iteration: 1,
    repeated_failure_count: 0,
    last_failure_fingerprint: null,
    last_observation: null,
    active_action_id: 'action-1',
    last_worker_summary: null,
    last_worker_stopped_at: null,
    created_at: 1,
    updated_at: 2,
  }
}

describe('AutopilotMissionBanner', () => {
  beforeEach(() => {
    missions = []
    startMutate.mockReset()
    pauseMutate.mockReset()
    resumeMutate.mockReset()
    stopMutate.mockReset()
  })

  it('opens the robot form from the floating session icon', () => {
    render(<AutopilotMissionBanner {...props()} />)

    fireEvent.click(
      screen.getByRole('button', {
        name: 'Open AutoPilot for session session-1',
      })
    )
    expect(screen.getByRole('dialog')).toHaveClass('overflow-y-auto')
    fireEvent.change(screen.getByLabelText('Mission'), {
      target: { value: 'Finish the orchestrator' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Start robot' }))

    expect(startMutate).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId: 'project-1',
        worktreeId: 'worktree-1',
        workerSessionId: 'session-1',
        worker: expect.objectContaining({
          backend: 'claude',
          model: 'claude-sonnet-5',
          surface: 'jean_chat',
          terminal_id: null,
        }),
        director: {
          backend: 'codex',
          model: 'gpt-5.4-mini',
          provider: null,
          effort: 'low',
        },
        startWorker: true,
      }),
      expect.any(Object)
    )
  })

  it('shows only the mission attached to the active session', () => {
    missions = [runningMission('other-session'), runningMission()]

    render(<AutopilotMissionBanner {...props()} />)

    fireEvent.click(
      screen.getByRole('button', {
        name: 'Open AutoPilot for session session-1',
      })
    )

    expect(screen.getByText('AutoPilot is attached')).toBeInTheDocument()
    expect(screen.getByText('Finish the orchestrator')).toBeInTheDocument()
    expect(
      screen.getByRole('button', { name: 'Pause AutoPilot' })
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Pause AutoPilot' }))
    expect(pauseMutate).toHaveBeenCalledWith('mission-1', expect.any(Object))
  })

  it('routes a native mission into the existing terminal', () => {
    render(
      <AutopilotMissionBanner
        {...props()}
        primarySurface="terminal"
        workerBackend="codex"
        workerModel="gpt-5.6-sol"
        terminalId="terminal-1"
      />
    )

    fireEvent.click(
      screen.getByRole('button', {
        name: 'Open AutoPilot for session session-1',
      })
    )
    fireEvent.change(screen.getByLabelText('Mission'), {
      target: { value: 'Fix the native session' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Start robot' }))

    expect(startMutate).toHaveBeenCalledWith(
      expect.objectContaining({
        worker: expect.objectContaining({
          backend: 'codex',
          surface: 'native_terminal',
          terminal_id: 'terminal-1',
        }),
      }),
      expect.any(Object)
    )
  })
})

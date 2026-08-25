import { useMemo, useState } from 'react'
import { Bot, Loader2, Pause, Play, Radio, Square } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { getBackendPlainLabel } from '@/components/ui/backend-label'
import {
  useAutopilotMissions,
  usePauseAutopilotMission,
  useResumeAutopilotMission,
  useStartAutopilotMission,
  useStopAutopilotMission,
} from '@/services/autopilot'
import {
  codexModelOptions,
  modelOptions,
  type CliBackend,
} from '@/types/preferences'
import type {
  AutopilotMission,
  StartAutopilotMissionInput,
} from '@/types/autopilot'
import {
  CURSOR_MODEL_OPTIONS,
  GROK_MODEL_OPTIONS,
  OPENCODE_MODEL_OPTIONS,
} from '@/components/chat/toolbar/toolbar-options'

const DIRECTOR_BACKENDS = [
  'claude',
  'codex',
  'opencode',
  'cursor',
  'grok',
] as const

type DirectorBackend = (typeof DIRECTOR_BACKENDS)[number]

const DIRECTOR_MODEL_OPTIONS: Record<
  DirectorBackend,
  { value: string; label: string }[]
> = {
  claude: modelOptions.map(option => ({
    value: option.value,
    label: option.label,
  })),
  codex: codexModelOptions.map(option => ({
    value: option.value,
    label: option.label,
  })),
  opencode: OPENCODE_MODEL_OPTIONS,
  cursor: CURSOR_MODEL_OPTIONS,
  grok: GROK_MODEL_OPTIONS,
}

const DEFAULT_DIRECTOR_MODELS: Record<DirectorBackend, string> = {
  claude: 'haiku',
  codex: 'gpt-5.4-mini',
  opencode: 'opencode/gpt-5.6-sol',
  cursor: 'cursor/auto',
  grok: 'grok/grok-4.5',
}

const TERMINAL_DIRECTOR_BACKENDS: CliBackend[] = ['claude', 'codex', 'opencode']

function isDirectorBackend(value: string): value is DirectorBackend {
  return DIRECTOR_BACKENDS.includes(value as DirectorBackend)
}

function formatLabel(value: string) {
  return value
    .split('_')
    .map(part => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ')
}

function defaultDirectorBackend(
  surface: 'jean_chat' | 'native_terminal',
  workerBackend: CliBackend
): DirectorBackend {
  if (surface === 'native_terminal' && isDirectorBackend(workerBackend)) {
    return workerBackend
  }
  return 'codex'
}

function findMission(
  missions: AutopilotMission[] | undefined,
  worktreeId: string,
  sessionId: string
) {
  return (
    (missions ?? [])
      .filter(
        candidate =>
          candidate.worktree_id === worktreeId &&
          candidate.worker_session_id === sessionId &&
          !['completed', 'failed', 'stopped'].includes(candidate.status)
      )
      .sort((left, right) => right.updated_at - left.updated_at)[0] ?? null
  )
}

export interface AutopilotMissionBannerProps {
  projectId: string | null
  worktreeId: string
  worktreePath: string
  sessionId: string
  workerBackend: CliBackend
  workerModel: string | null
  workerProvider: string | null
  primarySurface: 'chat' | 'terminal'
  terminalId?: string
}

export function AutopilotMissionBanner({
  projectId,
  worktreeId,
  worktreePath,
  sessionId,
  workerBackend,
  workerModel,
  workerProvider,
  primarySurface,
  terminalId,
}: AutopilotMissionBannerProps) {
  const { data: missions } = useAutopilotMissions()
  const start = useStartAutopilotMission()
  const pause = usePauseAutopilotMission()
  const resume = useResumeAutopilotMission()
  const stop = useStopAutopilotMission()
  const [open, setOpen] = useState(false)
  const [goal, setGoal] = useState('')
  const initialDirectorBackend = defaultDirectorBackend(
    primarySurface === 'terminal' ? 'native_terminal' : 'jean_chat',
    workerBackend
  )
  const [directorBackend, setDirectorBackend] = useState<DirectorBackend>(
    initialDirectorBackend
  )
  const [directorModel, setDirectorModel] = useState(
    DEFAULT_DIRECTOR_MODELS[initialDirectorBackend]
  )

  const mission = useMemo(
    () => findMission(missions, worktreeId, sessionId),
    [missions, sessionId, worktreeId]
  )
  const activeMutation =
    pause.isPending || resume.isPending || stop.isPending || start.isPending
  const modelOptionsForBackend = DIRECTOR_MODEL_OPTIONS[directorBackend]
  const modelOptionsWithCurrent = modelOptionsForBackend.some(
    option => option.value === directorModel
  )
    ? modelOptionsForBackend
    : [
        { value: directorModel, label: directorModel },
        ...modelOptionsForBackend,
      ]
  const workerSurface =
    primarySurface === 'terminal' ? 'native_terminal' : 'jean_chat'
  const workerSurfaceLabel =
    primarySurface === 'terminal'
      ? `${getBackendPlainLabel(workerBackend)} native TUI`
      : 'Jean Chat'
  const task = mission?.current_task_id
    ? mission.tasks.find(candidate => candidate.id === mission.current_task_id)
    : undefined

  const chooseDirectorBackend = (value: string) => {
    if (!isDirectorBackend(value)) return
    setDirectorBackend(value)
    setDirectorModel(DEFAULT_DIRECTOR_MODELS[value])
  }

  const runControl = (
    action: 'pause' | 'resume' | 'stop',
    mutate: (
      missionId: string,
      options: { onError: (error: unknown) => void }
    ) => void
  ) => {
    if (!mission) return
    mutate(mission.id, {
      onError: error =>
        toast.error(`Could not ${action} AutoPilot`, {
          description: error instanceof Error ? error.message : String(error),
        }),
    })
  }

  const submitMission = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const trimmedGoal = goal.trim()
    if (!trimmedGoal || !projectId || !worktreePath) return

    const input: StartAutopilotMissionInput = {
      projectId,
      worktreeId,
      worktreePath,
      goal: trimmedGoal,
      workerSessionId: sessionId,
      worker: {
        backend: workerBackend,
        model: workerModel,
        provider: workerProvider,
        execution_mode: 'yolo',
        surface: workerSurface,
        terminal_id:
          primarySurface === 'terminal' ? (terminalId ?? null) : null,
      },
      director: {
        backend: directorBackend,
        model: directorModel,
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
      startWorker: true,
    }

    start.mutate(input, {
      onSuccess: () => {
        setGoal('')
        setOpen(false)
        toast.success('AutoPilot started', {
          description: `The robot is attached to this ${workerSurfaceLabel} session.`,
        })
      },
      onError: error =>
        toast.error('Could not start AutoPilot', {
          description: error instanceof Error ? error.message : String(error),
        }),
    })
  }

  return (
    <div className="pointer-events-none absolute right-4 bottom-4 z-40">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            aria-label={`Open AutoPilot for session ${sessionId}`}
            title="AutoPilot"
            className="pointer-events-auto relative flex size-12 items-center justify-center rounded-full border border-primary/40 bg-gradient-to-br from-primary/90 via-primary to-violet-500 text-primary-foreground shadow-lg shadow-primary/25 transition-transform hover:scale-105 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
          >
            {mission?.director_running ? (
              <Loader2 className="size-5 animate-spin" />
            ) : (
              <Bot className="size-5" />
            )}
            {mission && (
              <span
                className="absolute right-0.5 bottom-0.5 size-3 rounded-full border-2 border-background bg-emerald-400"
                aria-label="AutoPilot is active"
              />
            )}
          </button>
        </PopoverTrigger>
        <PopoverContent
          side="top"
          align="end"
          sideOffset={10}
          className="max-h-[calc(100vh-24px)] w-[min(380px,calc(100vw-24px))] overflow-y-auto p-3"
        >
          {mission ? (
            <MissionDetails
              mission={mission}
              task={task}
              workerSurfaceLabel={workerSurfaceLabel}
              workerBackend={workerBackend}
              activeMutation={activeMutation}
              onPause={() => runControl('pause', pause.mutate)}
              onResume={() => runControl('resume', resume.mutate)}
              onStop={() => runControl('stop', stop.mutate)}
            />
          ) : (
            <form className="grid gap-3" onSubmit={submitMission}>
              <div>
                <div className="flex items-center gap-2 text-sm font-semibold">
                  <Bot className="size-4 text-primary" />
                  AutoPilot
                </div>
                <p className="mt-1 text-xs leading-5 text-muted-foreground">
                  This robot stays attached to the current session. It sends the
                  next bounded task into the same chat or TUI after each Worker
                  stop.
                </p>
              </div>

              <div className="grid gap-1.5">
                <label
                  htmlFor={`autopilot-goal-${sessionId}`}
                  className="text-xs font-medium"
                >
                  Mission
                </label>
                <Textarea
                  id={`autopilot-goal-${sessionId}`}
                  autoFocus
                  value={goal}
                  onChange={event => setGoal(event.target.value)}
                  placeholder="What should this session keep working toward?"
                  className="min-h-24 resize-y text-sm"
                  disabled={start.isPending}
                />
              </div>

              <div className="grid gap-1.5">
                <span className="text-xs font-medium">
                  Current Worker session
                </span>
                <div className="rounded-md border border-border/70 bg-muted/30 px-2.5 py-2 text-xs">
                  <div className="font-medium">{workerSurfaceLabel}</div>
                  <div className="mt-0.5 text-muted-foreground">
                    {getBackendPlainLabel(workerBackend)} -{' '}
                    {workerModel ?? 'backend default'}
                  </div>
                </div>
              </div>

              <div className="grid gap-1.5">
                <label
                  htmlFor={`autopilot-director-backend-${sessionId}`}
                  className="text-xs font-medium"
                >
                  AutoPilot backend
                </label>
                <Select
                  value={directorBackend}
                  onValueChange={chooseDirectorBackend}
                  disabled={
                    start.isPending ||
                    (primarySurface === 'terminal' &&
                      TERMINAL_DIRECTOR_BACKENDS.includes(workerBackend))
                  }
                >
                  <SelectTrigger
                    id={`autopilot-director-backend-${sessionId}`}
                    className="w-full"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {DIRECTOR_BACKENDS.map(backend => (
                      <SelectItem key={backend} value={backend}>
                        {getBackendPlainLabel(backend)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-[11px] leading-4 text-muted-foreground">
                  The robot uses its own cheap model for decisions; the Worker
                  remains in the current session.
                </p>
              </div>

              <div className="grid gap-1.5">
                <label
                  htmlFor={`autopilot-director-model-${sessionId}`}
                  className="text-xs font-medium"
                >
                  AutoPilot model
                </label>
                <Select
                  value={directorModel}
                  onValueChange={setDirectorModel}
                  disabled={start.isPending}
                >
                  <SelectTrigger
                    id={`autopilot-director-model-${sessionId}`}
                    className="w-full"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {modelOptionsWithCurrent.map(option => (
                      <SelectItem key={option.value} value={option.value}>
                        {option.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              {primarySurface === 'terminal' &&
                (workerBackend === 'opencode' || workerBackend === 'kimi') && (
                  <p className="rounded-md border border-amber-500/30 bg-amber-500/10 px-2.5 py-2 text-[11px] leading-4 text-amber-700 dark:text-amber-300">
                    Tasks are sent directly to this native TUI. Automatic
                    turn-boundary detection still needs a native lifecycle
                    signal for this backend; Claude and Codex are wired already.
                  </p>
                )}

              {!projectId && (
                <p className="text-xs text-destructive">
                  This session is not attached to a project yet.
                </p>
              )}

              <Button
                type="submit"
                disabled={
                  start.isPending ||
                  !goal.trim() ||
                  !projectId ||
                  !worktreePath ||
                  (primarySurface === 'terminal' && !terminalId)
                }
                className="w-full"
              >
                {start.isPending && <Loader2 className="size-4 animate-spin" />}
                Start robot
              </Button>
            </form>
          )}
        </PopoverContent>
      </Popover>
    </div>
  )
}

function MissionDetails({
  mission,
  task,
  workerSurfaceLabel,
  workerBackend,
  activeMutation,
  onPause,
  onResume,
  onStop,
}: {
  mission: AutopilotMission
  task?: AutopilotMission['tasks'][number]
  workerSurfaceLabel: string
  workerBackend: CliBackend
  activeMutation: boolean
  onPause: () => void
  onResume: () => void
  onStop: () => void
}) {
  return (
    <div className="grid gap-3">
      <div className="flex items-start gap-2">
        <div className="flex size-8 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
          <Bot className="size-4" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-sm font-semibold">AutoPilot is attached</div>
          <div className="truncate text-xs text-muted-foreground">
            {formatLabel(mission.status)}
            {mission.director_running ? ' - Director deciding' : ''}
          </div>
        </div>
        <div className="flex shrink-0 gap-1">
          {mission.status === 'running' && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="size-8"
              disabled={activeMutation}
              onClick={onPause}
              aria-label="Pause AutoPilot"
              title="Pause AutoPilot"
            >
              <Pause className="size-3.5" />
            </Button>
          )}
          {mission.status === 'paused' && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="size-8"
              disabled={activeMutation}
              onClick={onResume}
              aria-label="Resume AutoPilot"
              title="Resume AutoPilot"
            >
              <Play className="size-3.5" />
            </Button>
          )}
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-8 text-destructive hover:text-destructive"
            disabled={activeMutation}
            onClick={onStop}
            aria-label="Stop AutoPilot"
            title="Stop AutoPilot"
          >
            <Square className="size-3.5" />
          </Button>
        </div>
      </div>

      <div className="grid gap-2 rounded-md border border-border/70 bg-muted/20 px-2.5 py-2 text-xs">
        <div>
          <div className="text-muted-foreground">Mission</div>
          <div className="mt-0.5 whitespace-pre-wrap leading-5">
            {mission.goal}
          </div>
        </div>
        <div className="grid gap-1 sm:grid-cols-2">
          <div>
            <div className="text-muted-foreground">Worker TUI</div>
            <div className="font-medium">{workerSurfaceLabel}</div>
          </div>
          <div>
            <div className="text-muted-foreground">Worker backend</div>
            <div className="font-medium">
              {getBackendPlainLabel(workerBackend)}
            </div>
          </div>
          <div>
            <div className="text-muted-foreground">Mission phase</div>
            <div className="font-medium">
              {formatLabel(mission.phase)} - iteration {mission.iteration}/
              {mission.policy.max_iterations}
            </div>
          </div>
          <div>
            <div className="text-muted-foreground">Robot model</div>
            <div className="font-medium">
              {getBackendPlainLabel(mission.director.backend as CliBackend)} -{' '}
              {mission.director.model}
            </div>
          </div>
        </div>
      </div>

      <div className="flex gap-2 rounded-md bg-primary/5 px-2.5 py-2 text-[11px] leading-4 text-muted-foreground">
        <Radio className="mt-0.5 size-3.5 shrink-0 text-primary" />
        <span>
          The robot keeps the mission moving while every Worker message, edit,
          tool call, and check remains visible in this session.
        </span>
      </div>

      {task && (
        <div className="grid gap-1 text-xs">
          <span className="text-muted-foreground">Current task</span>
          <span className="whitespace-pre-wrap leading-5">
            {task.instruction}
          </span>
        </div>
      )}
    </div>
  )
}

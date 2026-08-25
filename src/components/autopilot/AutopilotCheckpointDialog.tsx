import { useMemo, useState } from 'react'
import { Bot, Check, CircleAlert, Pause } from 'lucide-react'
import { toast } from 'sonner'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import {
  useAutopilotMissions,
  useAutopilotUpdates,
  useRespondAutopilotApproval,
} from '@/services/autopilot'
import type { AutopilotMission, AutopilotCheckpoint } from '@/types/autopilot'

function pendingCheckpoint(mission: AutopilotMission) {
  return mission.grill_checkpoints.find(
    checkpoint => checkpoint.status === 'pending'
  )
}

export function AutopilotCheckpointDialog() {
  useAutopilotUpdates()
  const { data: missions } = useAutopilotMissions()
  const respond = useRespondAutopilotApproval()
  const [dismissedCheckpointId, setDismissedCheckpointId] = useState<
    string | null
  >(null)
  const [answerDraft, setAnswerDraft] = useState<{
    checkpointId: string
    value: string
  } | null>(null)

  const pending = useMemo(() => {
    const waiting = (missions ?? [])
      .filter(
        mission =>
          mission.status === 'waiting_for_human' && !mission.director_running
      )
      .map(mission => ({ mission, checkpoint: pendingCheckpoint(mission) }))
      .filter(
        (
          entry
        ): entry is {
          mission: AutopilotMission
          checkpoint: AutopilotCheckpoint
        } => entry.checkpoint !== undefined
      )
      .sort((left, right) => right.mission.updated_at - left.mission.updated_at)
    return waiting[0] ?? null
  }, [missions])

  const checkpoint = pending?.checkpoint ?? null
  const mission = pending?.mission ?? null
  const recommendedNextStep =
    checkpoint?.recommended_task_instruction?.trim() ||
    checkpoint?.recommended_answer ||
    ''
  const open = Boolean(checkpoint && checkpoint.id !== dismissedCheckpointId)
  const answer =
    checkpoint && answerDraft?.checkpointId === checkpoint.id
      ? answerDraft.value
      : recommendedNextStep

  const closeForLater = () => {
    if (checkpoint) setDismissedCheckpointId(checkpoint.id)
  }

  const submitAnswer = (value: string) => {
    if (!mission || !checkpoint || !value.trim()) return
    setDismissedCheckpointId(checkpoint.id)
    respond.mutate(
      {
        missionId: mission.id,
        checkpointId: checkpoint.id,
        answer: value.trim(),
        // A checkpoint marked for human review must remain human-approved even
        // when the text matches the Director recommendation.
        adoptRecommendation: false,
      },
      {
        onSuccess: () => {
          toast.success('Autopilot checkpoint accepted')
        },
        onError: error => {
          setDismissedCheckpointId(null)
          toast.error('Could not answer Autopilot checkpoint', {
            description: error instanceof Error ? error.message : String(error),
          })
        },
      }
    )
  }

  return (
    <Dialog
      open={open}
      onOpenChange={nextOpen => {
        if (!nextOpen) closeForLater()
      }}
    >
      <DialogContent className="w-[min(560px,calc(100vw-32px))] gap-4 p-5 sm:max-w-[560px]">
        <DialogHeader className="space-y-1 pr-6">
          <DialogTitle className="flex items-center gap-2 text-base">
            <Bot className="size-4 text-primary" />
            Autopilot needs one decision
          </DialogTitle>
          <DialogDescription className="text-xs leading-5">
            The Worker stopped at a bounded task boundary. The mission will not
            choose a new direction until this checkpoint is answered.
          </DialogDescription>
        </DialogHeader>

        {mission && checkpoint && (
          <div className="grid gap-3 text-sm">
            <div className="rounded-lg border border-border/70 bg-muted/25 p-3">
              <div className="mb-1 text-xs font-medium text-muted-foreground">
                Mission
              </div>
              <div className="font-medium">{mission.goal}</div>
            </div>

            <div className="grid gap-1.5">
              <div className="text-xs font-medium text-muted-foreground">
                Director question
              </div>
              <p className="leading-5">{checkpoint.question}</p>
            </div>

            <div className="grid gap-1.5 rounded-lg border border-primary/25 bg-primary/5 p-3">
              <div className="flex items-center gap-1.5 text-xs font-medium text-primary">
                <Check className="size-3.5" />
                Recommended next step
              </div>
              <p className="leading-5">{checkpoint.recommended_answer}</p>
              {checkpoint.why_it_matters && (
                <p className="text-xs leading-5 text-muted-foreground">
                  {checkpoint.why_it_matters}
                </p>
              )}
            </div>

            {checkpoint.evidence.length > 0 && (
              <div className="grid gap-1.5">
                <div className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
                  <CircleAlert className="size-3.5" />
                  Evidence
                </div>
                <ul className="grid gap-1 pl-5 text-xs leading-5 text-muted-foreground">
                  {checkpoint.evidence.map((item, index) => (
                    <li key={`${checkpoint.id}-evidence-${index}`}>{item}</li>
                  ))}
                </ul>
              </div>
            )}

            <div className="grid gap-1.5">
              <label
                htmlFor="autopilot-checkpoint-answer"
                className="text-xs font-medium"
              >
                Your answer
              </label>
              <Textarea
                id="autopilot-checkpoint-answer"
                value={answer}
                onChange={event =>
                  setAnswerDraft({
                    checkpointId: checkpoint.id,
                    value: event.target.value,
                  })
                }
                className="min-h-20 resize-y text-sm"
                disabled={respond.isPending}
              />
              <p className="text-xs text-muted-foreground">
                It starts filled with the recommendation; edit it if the project
                context requires a different next step.
              </p>
            </div>
          </div>
        )}

        <DialogFooter className="gap-2 sm:gap-2">
          <Button
            type="button"
            variant="ghost"
            onClick={closeForLater}
            disabled={respond.isPending}
          >
            <Pause className="size-4" />
            Later
          </Button>
          <Button
            type="button"
            variant="outline"
            onClick={() => submitAnswer(recommendedNextStep)}
            disabled={respond.isPending || !checkpoint}
          >
            {respond.isPending && (
              <span className="size-4 animate-spin">◌</span>
            )}
            Accept recommendation
          </Button>
          <Button
            type="button"
            onClick={() => submitAnswer(answer)}
            disabled={respond.isPending || !answer.trim() || !checkpoint}
          >
            Send answer
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

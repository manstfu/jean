import { useEffect } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { invoke, listen } from '@/lib/transport'
import { hasBackendTransport } from '@/lib/environment'
import type {
  AutopilotApprovalInput,
  AutopilotMission,
  AutopilotMissionEvent,
  AutopilotPolicy,
  StartAutopilotMissionInput,
} from '@/types/autopilot'

export const autopilotQueryKeys = {
  all: ['autopilot-missions'] as const,
  list: () => [...autopilotQueryKeys.all, 'list'] as const,
  detail: (missionId: string) =>
    [...autopilotQueryKeys.all, 'detail', missionId] as const,
  events: (missionId: string) =>
    [...autopilotQueryKeys.detail(missionId), 'events'] as const,
}

export function useAutopilotMissions() {
  return useQuery({
    queryKey: autopilotQueryKeys.list(),
    queryFn: () => invoke<AutopilotMission[]>('list_autopilot_missions'),
    enabled: hasBackendTransport(),
    staleTime: 10_000,
  })
}

export function useAutopilotMission(missionId: string | null) {
  return useQuery({
    queryKey: autopilotQueryKeys.detail(missionId ?? ''),
    queryFn: () =>
      invoke<AutopilotMission>('get_autopilot_mission', { missionId }),
    enabled: Boolean(missionId) && hasBackendTransport(),
  })
}

export function useAutopilotMissionEvents(missionId: string | null) {
  return useQuery({
    queryKey: autopilotQueryKeys.events(missionId ?? ''),
    queryFn: () =>
      invoke<AutopilotMissionEvent[]>('get_autopilot_mission_events', {
        missionId,
      }),
    enabled: Boolean(missionId) && hasBackendTransport(),
  })
}

export function useAutopilotUpdates() {
  const queryClient = useQueryClient()

  useEffect(() => {
    if (!hasBackendTransport()) return

    let disposed = false
    let unlisten: (() => void) | undefined

    void listen<{ mission_id?: string }>('autopilot:updated', event => {
      const missionId = event.payload?.mission_id
      if (missionId) {
        queryClient.invalidateQueries({
          queryKey: autopilotQueryKeys.detail(missionId),
        })
        queryClient.invalidateQueries({
          queryKey: autopilotQueryKeys.events(missionId),
        })
      }
      queryClient.invalidateQueries({ queryKey: autopilotQueryKeys.list() })
    }).then(stop => {
      if (disposed) {
        stop()
      } else {
        unlisten = stop
      }
    })

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [queryClient])
}

export function useStartAutopilotMission() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (input: StartAutopilotMissionInput) =>
      invoke<AutopilotMission>('start_autopilot_mission', input),
    onSuccess: mission => {
      queryClient.setQueryData(autopilotQueryKeys.detail(mission.id), mission)
      queryClient.invalidateQueries({ queryKey: autopilotQueryKeys.list() })
    },
  })
}

export function useStartAutopilotWorker() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (missionId: string) =>
      invoke<AutopilotMission>('start_autopilot_worker', { missionId }),
    onSuccess: mission => {
      queryClient.setQueryData(autopilotQueryKeys.detail(mission.id), mission)
      queryClient.invalidateQueries({ queryKey: autopilotQueryKeys.list() })
    },
  })
}

export function useRunAutopilotDirector() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (missionId: string) =>
      invoke<AutopilotMission>('run_autopilot_director', { missionId }),
    onSuccess: mission => {
      queryClient.setQueryData(autopilotQueryKeys.detail(mission.id), mission)
      queryClient.invalidateQueries({ queryKey: autopilotQueryKeys.list() })
      queryClient.invalidateQueries({
        queryKey: autopilotQueryKeys.events(mission.id),
      })
    },
  })
}

function useMissionControlMutation(
  command:
    | 'pause_autopilot_mission'
    | 'resume_autopilot_mission'
    | 'stop_autopilot_mission'
) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (missionId: string) =>
      invoke<AutopilotMission>(command, { missionId }),
    onSuccess: mission => {
      queryClient.setQueryData(autopilotQueryKeys.detail(mission.id), mission)
      queryClient.invalidateQueries({ queryKey: autopilotQueryKeys.list() })
    },
  })
}

export const usePauseAutopilotMission = () =>
  useMissionControlMutation('pause_autopilot_mission')

export const useResumeAutopilotMission = () =>
  useMissionControlMutation('resume_autopilot_mission')

export const useStopAutopilotMission = () =>
  useMissionControlMutation('stop_autopilot_mission')

export function useRespondAutopilotApproval() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (input: AutopilotApprovalInput) =>
      invoke<AutopilotMission>('respond_autopilot_approval', input),
    onSuccess: mission => {
      queryClient.setQueryData(autopilotQueryKeys.detail(mission.id), mission)
      queryClient.invalidateQueries({ queryKey: autopilotQueryKeys.list() })
      queryClient.invalidateQueries({
        queryKey: autopilotQueryKeys.events(mission.id),
      })
    },
  })
}

export function useUpdateAutopilotPolicy() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({
      missionId,
      policy,
    }: {
      missionId: string
      policy: AutopilotPolicy
    }) =>
      invoke<AutopilotMission>('update_autopilot_policy', {
        missionId,
        policy,
      }),
    onSuccess: mission => {
      queryClient.setQueryData(autopilotQueryKeys.detail(mission.id), mission)
      queryClient.invalidateQueries({ queryKey: autopilotQueryKeys.list() })
    },
  })
}

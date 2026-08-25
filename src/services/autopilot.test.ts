import { describe, expect, it } from 'vitest'
import { autopilotQueryKeys } from './autopilot'

describe('autopilot service', () => {
  it('keeps mission queries scoped under one cache root', () => {
    expect(autopilotQueryKeys.all).toEqual(['autopilot-missions'])
    expect(autopilotQueryKeys.list()).toEqual(['autopilot-missions', 'list'])
    expect(autopilotQueryKeys.detail('mission-1')).toEqual([
      'autopilot-missions',
      'detail',
      'mission-1',
    ])
    expect(autopilotQueryKeys.events('mission-1')).toEqual([
      'autopilot-missions',
      'detail',
      'mission-1',
      'events',
    ])
  })
})

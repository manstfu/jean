# Task: Add Autopilot Orchestrator

## Goal

Add a user-started continuous Autopilot mode that uses a cheap Director model
to keep a coding Worker moving through planning, implementation, verification,
debugging, review, and completion. At every Worker stop, run a grill checkpoint
that pressure-tests the direction, proposes a recommended answer, and either
continues safely or pauses for the user.

Autopilot must work with the visible Jean Chat Worker and Jean's Claude, Codex,
OpenCode, Kimi, and future backend adapters. It is independent from Mr. Robot.

Design: [Autopilot Orchestrator Design](../superpowers/specs/2026-08-25-autopilot-orchestrator-design.md)

Grill checkpoint reference: [grill-me SKILL.md](https://github.com/JuliusBrussee/skills/blob/main/skills/grill-me/SKILL.md)

## Scope boundary

- Do not edit `jean-core/src/auto_fix/`.
- Do not edit Mr. Robot settings, scheduler, worktree origin, or issue sweep
  behavior.
- Do not make Mr. Robot an Autopilot entry point in this task.
- Reuse generic Jean session, approval, review, checkpoint, and terminal
  infrastructure only through stable interfaces.

## Product modes

- [x] Keep Conversation as a normal one-turn mode.
- [x] Keep Yolo as a normal one-turn autonomous mode.
- [x] Add Autopilot as a persistent mission mode, normally using a yolo Worker
      policy but with configurable safe/human-gated policies.
- [x] Keep AutoPilot out of the New Session picker and attach one floating robot
      to each open session instead.
- [x] Inherit the current session's chat/native TUI, backend, model, provider,
      and managed terminal handle when a robot starts a mission.

## Phase 0 — contracts and persistence

- [x] Add the initial `jean-core/src/autopilot/` contracts, storage,
      controller, and Director modules. Policy/adapter modules remain
      intentionally behind the controller boundary.
- [x] Define persisted `Mission`, `MissionTask`, `MissionPolicy`,
      `MissionObservation`, `DirectorDecision`, grill checkpoints, phase,
      status, and Worker configuration types.
- [x] Use snake_case persisted fields, `serde(default)`, schema versioning,
      and app-data storage separate from Mr. Robot.
- [x] Persist an append-only mission event/decision log.
- [x] Add an in-process mission lock and `active_action_id` for idempotent
      transitions within native and WebSocket clients. Cross-process recovery
      still needs a persisted lease.
- [ ] Add startup rehydration for running, waiting, and resumable missions.
- [ ] Add Rust transition, migration, recovery, duplicate-action, and
      repeated-failure tests.

## Phase 1 — Director model and decision engine

- [x] Move Director backend/model selection into the session-scoped robot. Keep
      legacy preference fallbacks only for older persisted missions.
- [ ] Define model precedence: mission override → project/default preference →
      installed/catalog fallback.
- [ ] Prefer a fast low-cost model by default without hardcoding provider
      pricing or assuming a backend is installed.
- [x] Define a headless Director contract with strict structured JSON parsing
      and safe Claude/Codex/Cursor/Grok/OpenCode invocation/extraction.
- [x] Ensure supported Director adapters receive no arbitrary file-editing or
      shell tools; unsupported adapters fail closed.
- [x] Build bounded observations from mission state, worker summary, status,
      git changes, checks, reviews, approvals, and recent decisions.
- [x] Preserve the Worker handoff: status, an optional suggested next task,
      and an explicit mission-complete marker for the Director.
- [ ] Include grill-checkpoint context: unresolved decisions, assumptions,
      risks, dependencies, validation, rollback, and evidence already checked.
- [ ] Validate every Director action in Rust against mission policy, current
      phase, worker capabilities, worktree scope, and budgets.
- [x] Require the Director to emit at most one checkpoint question at a time,
      with a recommended answer and one-sentence rationale.
- [x] Add focused tests for valid/invalid JSON, unsupported actions, oversized
      output, schema validation, and policy rejection.

## Phase 2 — Worker adapters

### Jean Chat adapter

- [x] Start a Jean Chat Worker session using the existing persisted queue/session
      infrastructure.
- [x] Send one bounded task at a time through the persisted queue/session APIs.
- [x] Observe turn completion, errors, and cancellation through lifecycle
      events; plan/permission/resume mapping remains pending.
- [ ] Map plan approval, permission responses, cancellation, and resume into
      the common adapter contract.

### Native AI terminal compatibility adapter

- [x] Define the common adapter boundary: start, send one bounded task, observe,
      approve, deny, cancel, and capability discovery.
- [x] Implement provider-native Claude Worker support using structured CLI
      output and configured permission mode.
- [x] Implement provider-native Codex Worker support using its structured
      `codex exec` output schema and worktree sandbox.
- [x] Implement provider-native OpenCode Worker support using its structured
      one-shot API/output boundary.
- [x] Implement provider-native Kimi Worker support using its one-shot CLI
      protocol and yolo flag mapping.
- [ ] Define per-backend capability flags for plans, permissions, resume,
      cancellation, usage, images, and structured tool events.
- [x] Send native prompts through the existing Jean-managed terminal handle;
      do not scrape ANSI screen contents or create a second terminal.
- [x] Wire Claude native `Stop`/`StopFailure` hooks and the existing Codex
      notification into the shared terminal lifecycle boundary; OpenCode/Kimi
      native stop signals remain pending.
- [x] Keep the visible Worker in the existing Jean Chat/TUI surface so the user
      can inspect messages, tools, edits, diffs, and checks. The Director never
      creates a second visible terminal.
- [x] Attach a mission to the existing managed native terminal surface when the
      active session has one; arbitrary external PTYs remain unsupported.
- [ ] If an existing manually opened terminal cannot be safely adopted, show a
      clear message and offer a dedicated managed Worker session.
- [x] Add structured Worker output validation and native mission lifecycle tests
      for the current provider-native adapters.

## Phase 3 — mission loop

- [x] Add mission creation with goal, acceptance criteria, constraints,
      non-goals, Worker selection, Director selection, policy, and limits.
- [x] Represent `premortem → plan → implement → verify → grill → review → fix`
      phases in the persisted state machine.
- [ ] Generate premortem risks, detection checks, and mitigation tasks before
      the first Worker implementation task.
- [ ] Trigger the controller from worker completion, plan/permission requests,
      check/review completion, errors, cancellation, and recovery events.
- [x] Run a persisted grill checkpoint when an attached Jean Chat or native
      Worker task/turn stops.
- [ ] Inspect repository, mission, logs, diff, checks, and review evidence
      before treating a checkpoint question as unresolved.
- [x] Persist each checkpoint question, recommended answer, evidence,
      adopted/human answer, and resulting action.
- [x] Gate recommendation adoption on explicit safe-action policy and
      checkpoint resolvability; automatic task launch after a resolved
      checkpoint is implemented for missions that opted into Worker start.
- [x] Automatically adopt typed, mechanically resolvable recommendations only
      when the mission policy allows the action; completion additionally needs
      the Worker mission-complete marker and a Director decision.
- [ ] Pause and ask the user when the recommendation requires product
      judgment, changes scope, or conflicts with constraints.
- [x] Ask the Director for exactly one next action at the Jean Chat Worker
      boundary.
- [x] Let the Worker suggest the next task, then have the Director refine it
      with premortem, evidence, and focused verification before resending it.
- [x] Automatically create adaptive follow-up and debug tasks from a safe
      Director instruction.
- [ ] Run configured tests/checks and optionally the existing Jean review job.
- [ ] Determine completion from acceptance evidence, not Worker prose alone.
- [ ] Stop on completion, human gate, cancellation, budget exhaustion,
      missing capability, or repeated identical failure.
- [ ] Ensure one controller transition cannot send duplicate Worker prompts.
- [ ] Add Rust state-machine integration tests for success, debug loops,
      approval pauses, cancellation, crash recovery, and retry limits.

## Phase 4 — policy and approvals

- [ ] Define safe, yolo, and human-gated policy presets.
- [ ] Auto-allow only configured low-risk actions in safe mode: reads,
      searches, tests, and worktree-scoped edits.
- [ ] Require human approval for push, merge, delete, production changes,
      migrations, credentials, and other configured high-risk actions.
- [ ] Bridge existing plan, Claude permission, Codex command, OpenCode
      permission, question, MCP elicitation, and checkpoint approval flows.
- [ ] Persist pending approval state and recover it after restart.
- [ ] Add tests proving the Director cannot bypass policy through prompt text.

## Phase 5 — frontend and notifications

- [x] Add `src/types/autopilot.ts` and service/query hooks.
- [x] Add a floating session-scoped robot with mission and Director model form.
- [x] Remove AutoPilot controls from General Settings and the New Session picker.
- [ ] Add a full mission panel showing status, phase, current task, Worker,
      Director, iteration count, last decision, approvals, and limits. A
      floating in-chat assistant is implemented for the active worktree.
- [x] Add pause, resume, stop, and checkpoint answer controls. Deny/retry
      semantics remain pending.
- [x] Subscribe to `autopilot:*` events and invalidate mission queries.
- [ ] Use background-operation toasts for start/stop/failure/completion.
- [ ] Add native desktop, web access, and mobile-safe affordances where the UI
      is shared.
- [ ] Add frontend tests for settings precedence, mode selection, status
      transitions, approval rendering, and mission recovery.

## Phase 6 — command and WebSocket integration

- [x] Register the initial Autopilot commands through the shared core dispatch
      path.
- [x] Add the initial commands to `jean-core/src/http_server/dispatch.rs` with
      dual
      camelCase/snake_case extraction and mutation invalidation where needed.
- [x] Verify native Tauri and WebSocket/web access use the same shared
      dispatch path for mission start, status, control, and approval operations.
- [ ] Verify native Tauri and WebSocket/web access both support start, status,
      control, and approval operations.
- [ ] Add event replay or query-based recovery so UI reloads do not lose mission
      progress.

## Phase 7 — verification and documentation

- [ ] Add E2E coverage for Conversation/Yolo plus the session-scoped robot.
- [ ] Add E2E coverage for Jean Chat and at least one managed native backend.
- [ ] Add restart/reconnect coverage with a running Worker and pending
      Director decision.
- [x] Document session-scoped Worker routing and native terminal compatibility
      behavior.
- [ ] Document safety policy, budgets, human gates, and stop conditions.
- [ ] Update relevant developer architecture documentation after the design is
      implemented.
- [ ] Run `bun run check:all`.
- [x] Keep the implementation independent from `jean-core/src/auto_fix/`;
      Mr. Robot behavior and settings were not changed.

## Acceptance criteria

1. A user can open the AutoPilot robot from any active session after choosing
   Conversation or Yolo.
2. A user can choose the Director backend/model in that session's robot, with a
   backend-matched model list.
3. A mission persists its direction, tasks, decisions, approvals, and status.
4. The Director runs headlessly; it does not create a second visible terminal.
5. Jean Chat can execute a mission end to end.
6. Claude, Codex, OpenCode, Kimi, and other installed backend adapters can run
   as a visible Worker without screen scraping; native terminal compatibility
   remains a separate managed-surface follow-up.
7. Autopilot automatically follows a completed task with verification or the
   next task, and creates debug tasks when checks fail.
8. Every Worker stop creates a grill checkpoint with a question, recommended
   answer, evidence, and adopted/human answer.
9. Product decisions are surfaced to the user instead of being guessed.
10. Premortem and review steps are represented in mission history.
11. High-risk actions pause for a human unless explicitly enabled by policy.
12. Restarting Jean or disconnecting Web Access does not duplicate work.
13. Mr. Robot code, settings, scheduler, and issue workflow are unchanged.

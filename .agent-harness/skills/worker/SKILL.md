---
name: worker
description: Team worker protocol (ACK, mailbox, task lifecycle) for coordinator-native Harness teams
---

# Worker Skill

This skill is for a Codex session that was started as an Harness Team worker (a native workflow surface spawned by `$team`).

## Identity

You MUST be running with `HARNESS_TEAM_SETTING_WORKER` set. It looks like:

`<team-name>/worker-<n>`

Example: `alpha/worker-2`

## Load Worker Skill Path (Claude/Codex)

When a worker inbox tells you to load this skill, resolve the first existing path:

1. `${CODEX_HOME:-~/.codex}/skills/worker/SKILL.md`
2. `~/.codex/skills/worker/SKILL.md`
3. `<leader_cwd>/.codex/skills/worker/SKILL.md`
4. `<leader_cwd>/skills/worker/SKILL.md` (repo fallback)

## Startup Protocol (ACK)

1. Parse `HARNESS_TEAM_SETTING_WORKER` into:
   - `teamName` (before the `/`)
   - `workerName` (after the `/`, usually `worker-<n>`)
2. Send a startup ACK to the lead mailbox **before task work**:
   - Recipient worker id: `leader-fixed`
   - Body: one short deterministic line (recommended: `ACK: <workerName> initialized`).
3. After ACK, proceed to your inbox instructions.

The lead will see your message in:

`<team_state_root>/team/<teamName>/mailbox/leader-fixed.json`

Use CLI interop:
- `native team tools api send-message --input <json> --json` with `{team_name, from_worker, to_worker:"leader-fixed", body}`

Copy/paste template:

```bash
native team tools api send-message --input "{\"team_name\":\"<teamName>\",\"from_worker\":\"<workerName>\",\"to_worker\":\"leader-fixed\",\"body\":\"ACK: <workerName> initialized\"}" --json
```

## Inbox + Tasks

1. Resolve canonical team state root in this order:
   1) `HARNESS_TEAM_SETTING_STATE_ROOT` env
   2) worker identity `team_state_root`
   3) team config/manifest `team_state_root`
   4) local cwd fallback (`Harness workflow projection`)
2. Read your inbox:
   `<team_state_root>/team/<teamName>/workers/<workerName>/inbox.md`
3. Pick the first unblocked task assigned to you.
4. Read the task file:
   `<team_state_root>/team/<teamName>/tasks/task-<id>.json` (example: `task-1.json`)
5. Task id format:
   - The MCP/state API uses the numeric id (`"1"`), not `"task-1"`.
   - Never use legacy `tasks/{id}.json` wording.
6. Claim the task (do NOT start work without a claim) using claim-safe lifecycle CLI interop (`native team tools api claim-task --json`).
7. Do the work.
8. Complete/fail the task via lifecycle transition CLI interop (`native team tools api transition-task-status --json`) from `in_progress` to `completed` or `failed`.
   - Do NOT directly write lifecycle fields (`status`, `owner`, `result`, `error`) in task files.
9. Use `native team tools api release-task-claim --json` only for rollback/requeue to `pending` (not for completion).
10. Update your worker status:
   `<team_state_root>/team/<teamName>/workers/<workerName>/status.json` with `{"state":"idle", ...}`

## Mailbox

Check your mailbox for messages:

`<team_state_root>/team/<teamName>/mailbox/<workerName>.json`

When notified, read messages and follow any instructions. Use short ACK replies when appropriate.

Note: leader dispatch is state-first. The durable queue lives at:
`<team_state_root>/team/<teamName>/dispatch/requests.json`
Hooks/watchers may nudge you after mailbox/inbox state is already written.

Use CLI interop:
- `native team tools api mailbox-list --json` to read
- `native team tools api mailbox-mark-delivered --json` to acknowledge delivery

Copy/paste templates:

```bash
native team tools api mailbox-list --input "{\"team_name\":\"<teamName>\",\"worker\":\"<workerName>\"}" --json
native team tools api mailbox-mark-delivered --input "{\"team_name\":\"<teamName>\",\"worker\":\"<workerName>\",\"message_id\":\"<MESSAGE_ID>\"}" --json
```

## Dispatch Discipline (state-first)

Worker sessions should treat team state + CLI interop as the source of truth.

- Prefer inbox/mailbox/task state and `native team tools api ... --json` operations.
- Do **not** rely on ad-hoc native terminal UI keystrokes as a primary delivery channel.
- If a manual trigger arrives (for example `coordinator message routing` nudge), treat it only as a prompt to re-check state and continue through the normal claim-safe lifecycle.

## Shutdown

If the lead sends a shutdown request, follow the shutdown inbox instructions exactly, write your shutdown ack file, then exit the Codex session.

## Harness substrate override

When this skill is loaded by `agent-harness`, the workflow protocol above is the behavioral source, but the runtime substrate differs from Harness:

- Use coordinator-owned workflow events, workflow projections, task records, and evidence artifacts as the authority.
- Do **not** write or mutate per-mode `Harness workflow projection/*.json` files; lifecycle, phase, continuation, and closeout state are event-sourced by the harness.
- Translate Harness CLI/state operations to harness-native surfaces when needed: workflow evidence/status/goal/wiki CLI commands, native `task`/team tools, and workflow projections.
- Treat native terminal UI-specific Harness team/question instructions as conceptual guidance unless the harness exposes an equivalent native tool; prefer the harness native tool surface.
- Keep final claims evidence-backed: changed files, commands run, artifacts/evidence refs, remaining risks, and the stop condition reached.

## Harness state contract

Harness workflow state is authoritative through coordinator-owned events, workflow projections, native tool artifacts, and recorded workflow evidence. Skills must not require external state files, terminal-pane routing, or upstream CLI lifecycle commands as proof of progress.

## Execution protocol

Use the native Harness command dispatch, question, team, task, evidence, and verification surfaces named by the active workflow. Treat compatibility references as historical context only, and translate them into coordinator-owned actions before acting.

## Evidence and closeout contract

Record material progress as workflow evidence with artifact paths or command output summaries. Close only after the relevant checks pass, pending tasks are resolved or explicitly aborted, and the operator-facing status can be replayed from Harness events.

## Stop/escalation conditions

Stop when the workflow objective is verified complete, cancelled by the operator, or blocked by missing authority. Escalate only for destructive, credentialed, external-production, or materially scope-changing choices.

## Verification checklist

- Native Harness workflow projection reflects the expected mode/status.
- Required evidence artifacts or command summaries are recorded.
- Targeted tests, lint, docs checks, or visual/review gates named by the workflow have fresh results.
- No external state-file, terminal multiplexer, or upstream CLI command is the proof boundary.

## Purpose

Provide a native Harness workflow protocol for this skill so command dispatch, state projection, evidence, and closeout remain coordinator-owned and replayable.

## Use when

Use this skill when the matching `$` workflow command or catalog entry is selected and the operator request fits the workflow description.

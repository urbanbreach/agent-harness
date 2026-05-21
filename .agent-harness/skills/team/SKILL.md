---
name: team
description: N coordinated agents on shared task list using coordinator-native orchestration
---

# Team Skill

`$team` is the coordinator-native parallel execution mode for Harness. It starts real worker Codex and/or Claude CLI sessions in split panes and coordinates them through `Harness workflow projection/team/...` files plus CLI team interop (`Harness team coordination API ...`) and state files.

This skill is operationally sensitive. Treat it as an operator workflow, not a generic prompt pattern. In Codex App or plain outside-runtime sessions, do not present `$team` / `native team tools` as directly available; launch Harness CLI from shell first, or stay on the nearest app-safe surface until the user explicitly wants the Harness runtime.

## Team vs Native Subagents

- Use **Codex native subagents** for bounded, in-session parallelism where one leader thread can fan out a few independent subtasks and wait for them directly.
- Use **`native team tools`** when you need durable native workers, shared task state, mailbox/dispatch coordination, worktrees, explicit lifecycle control, or long-running parallel execution that must survive beyond one local reasoning burst.
- Native subagents can complement team/ralph execution, but they do **not** replace the native terminal UI team runtime's stateful coordination contract.

## What This Skill Must Do

## GPT-5.5 Guidance Alignment

Use the shared workflow guidance pattern: outcome-first framing, concise visible updates for multi-step work, local overrides for the active workflow branch, validation proportional to risk, explicit stop rules, and automatic continuation for safe reversible steps. Ask only for material, destructive, credentialed, external-production, or preference-dependent branches.

When user triggers `$team`, the agent must:

1. Invoke Harness runtime directly with `native team tools ...`
2. Avoid replacing the flow with in-process `spawn_agent` fanout
3. Verify startup and surface concrete state/pane evidence
4. If active team mode state is missing, initialize/sync it from canonical team runtime state before proceeding
5. Keep team state alive until workers are terminal (unless explicit abort)
6. Handle cleanup and stale-pane recovery when needed

If `native team tools` is unavailable, stop with a hard error.

## Invocation Contract

```bash
native team tools [N:agent-type] "<task description>"
```

Examples:

```bash
native team tools 3:executor "analyze feature X and report flaws"
native team tools "debug flaky integration tests"
native team tools "ship end-to-end fix with verification"
```

### Team-first launch contract

`native team tools ...` is now the canonical launch path for coordinated execution.
Team mode should carry its own parallel delivery + verification lanes without
requiring a separate linked Ralph launch up front.

- **Canonical launch:** use plain `native team tools ...` / `$team ...` for coordinated workers.
- **Verification ownership:** keep one lane focused on tests, regression coverage, and evidence before shutdown.
- **Escalation:** start a separate `$ralph ...` / `$ralph ...` only when a later manual follow-up still needs a persistent single-owner fix/verification loop.
- **Deprecation:** `native team tools ralph ...` has been removed. Use plain `native team tools ...` for team execution or run `$ralph ...` separately when you explicitly want a later Ralph loop.

### Team + Ultragoal bridge

Use `$ultragoal` for durable leader-owned goal/ledger tracking and `$team` for parallel execution lanes. When Team is launched with an active `Harness goal ledger artifacts/goals.json`, worker inboxes/status may include leader-owned Ultragoal context: `Harness goal ledger artifacts/goals.json`, `Harness goal ledger artifacts/ledger.jsonl`, the active goal id, external goal context, and the `fresh_leader_get_goal_required` checkpoint policy.

Workers provide task status and verification evidence only. They do not own Ultragoal goal state, create worker ledgers, mutate `Harness goal ledger artifacts`, auto-launch Team from Ultragoal, or perform hidden Codex goal mutation. The leader uses terminal Team evidence plus a fresh `get_goal` snapshot to run `harness workflow goal-ledger checkpoint --goal-id <id> --status complete --evidence "<team evidence mentioning Harness goal ledger artifacts and <id>>" --codex-goal-json <fresh-get_goal-json-or-path>`.

### Claude teammates (v0.6.0+)

Important: `N:agent-type` (for example `2:executor`) selects the **worker role prompt**, not the worker CLI (`codex` vs `claude`).

To launch Claude teammates, use the team worker CLI env vars:

```bash
# Force all teammates to Claude CLI
HARNESS_TEAM_SETTING_WORKER_CLI=claude native team tools 2:executor "update docs and report"

# Mixed team (worker 1 = Codex, worker 2 = Claude)
HARNESS_TEAM_SETTING_WORKER_CLI_MAP=codex,claude native team tools 2:executor "split doc/code tasks"

# Auto mode: Claude is selected when worker launch args/model contains 'claude'
HARNESS_TEAM_SETTING_WORKER_CLI=auto HARNESS_TEAM_SETTING_WORKER_LAUNCH_ARGS="--model claude-..." native team tools 2:executor "run mixed validation"
```

## Preconditions

Before running `$team`, confirm:

1. `native terminal UI` installed (`native terminal UI -V`)
2. Current leader session is inside the Harness runtime (`$HARNESS_TERMINAL` is set)
3. `harness` command resolves to the intended install/build
4. If running repo-local `harness ...`, run `npm run build` after `src` changes
5. Check HUD pane count in the leader window and avoid duplicate `hud --watch` panes before split

Suggested preflight:

```bash
native terminal UI list-panes -F '#{pane_id}\t#{pane_start_command}' | rg 'hud --watch' || true
```

If duplicates exist, remove extras before `native team tools` to prevent HUD ending up in worker stack.

## Pre-context Intake Gate

Before launching `native team tools`, require a grounded context snapshot:

1. Derive a task slug from the request.
2. Reuse the latest relevant snapshot in `target/harness-artifacts/context/{slug}-*.md` when available.
3. If none exists, create `target/harness-artifacts/context/{slug}-{timestamp}.md` (UTC `YYYYMMDDTHHMMSSZ`) with:
   - task statement
   - desired outcome
   - known facts/evidence
   - constraints
   - unknowns/open questions
   - likely codebase touchpoints
4. If ambiguity remains high, run `explore` first for brownfield facts, then run `$deep-interview --quick <task>` before team launch.
5. If current correctness depends on official docs, version-aware framework guidance, best practices, or external dependency behavior, auto-delegate `researcher` as an evidence lane before or alongside worker launch instead of relying on repo-local recall alone.

Do not start worker panes until this gate is satisfied; if forced to proceed quickly, state explicit scope/risk limitations in the launch report.

For simple read-only brownfield lookups during intake, follow active session guidance: when `USE_Harness_EXPLORE_CMD` is enabled, prefer `harness codesearch/explore` with narrow, concrete prompts; otherwise use the richer normal explore path and fall back normally if `harness codesearch/explore` is unavailable.

## Follow-up Staffing Contract

When `$team` is used as a follow-up mode from ralplan, carry forward the approved plan's explicit **available-agent-types roster** and convert it into concrete staffing guidance before launch:

- keep worker-role choices inside the known roster
- state the recommended headcount and role counts
- state the suggested reasoning level for each lane when available
- explain why each lane exists (delivery, verification, specialist support)
- include an explicit launch hint (`native team tools N "<task>"` / `$team N "<task>"`) for the coordinated team run; mention a later separate Ralph follow-up only when genuinely needed
- if the ideal role is unavailable, choose the closest role from the roster and say so

## Current Runtime Behavior (As Implemented)

`native team tools` currently performs:

1. Parse args (`N`, `agent-type`, task)
2. Sanitize team name from task text
3. Initialize team state:
   - `Harness workflow projection/team/<team>/config.json`
   - `Harness workflow projection/team/<team>/manifest.v2.json`
   - `Harness workflow projection/team/<team>/tasks/task-<id>.json`
4. Compose team-scoped worker instructions file at:
   - `Harness workflow projection/team/<team>/worker-agents.md`
   - Uses project `AGENTS.md` content (if present) + worker overlay, without mutating project `AGENTS.md`
5. Resolve canonical shared state root from leader cwd (`<leader-cwd>/Harness workflow projection`)
6. Split current native team view into worker panes
7. Launch workers with:
   - `HARNESS_TEAM_SETTING_WORKER=<team>/worker-<n>`
   - `HARNESS_TEAM_SETTING_STATE_ROOT=<leader-cwd>/Harness workflow projection`
   - `HARNESS_TEAM_SETTING_LEADER_CWD=<leader-cwd>`
   - worker CLI selected by `HARNESS_TEAM_SETTING_WORKER_CLI` / `HARNESS_TEAM_SETTING_WORKER_CLI_MAP` (`codex` or `claude`)
   - optional worktree metadata envs when `--worktree` is used
7. Wait for worker readiness (`capture-pane` polling)
8. Write per-worker `inbox.md` and trigger via `coordinator message routing`
9. Return control to leader; follow-up uses `status` / `resume` / `shutdown`

If coarse active team mode state is missing while canonical team runtime state exists, restore/sync the active team mode state before relying on hook/mode-aware behavior.

Important:

- Leader remains in existing pane
- Worker panes are independent full Codex/Claude CLI sessions
- Workers may run in separate git worktrees (`native team tools --worktree[=<name>]`) while sharing one team state root
- Worker ACKs go to `mailbox/leader-fixed.json`
- Notify hook updates worker heartbeat and sends lifecycle-driven leader nudges (for example resolved native worker Stop/all-idle or stale-leader evidence) during active team mode; deprecated worker stall/progress heuristics are not operator-facing guidance.
- Submit routing uses this CLI resolution order per worker trigger:
  1) explicit worker CLI provided by runtime state (persisted on worker identity/config),
  2) `HARNESS_TEAM_SETTING_WORKER_CLI_MAP` entry for that worker index,
  3) fallback `HARNESS_TEAM_SETTING_WORKER_CLI` / auto detection.
- Mixed CLI-map teams are supported for both startup and trigger submit behavior.
- Trigger submit differs by CLI:
  - Codex may use queue-first `Tab` on busy panes (strategy-dependent).
  - Claude always uses direct Enter-only (`C-m`) rounds (never queue-first `Tab`).

### Team worker model + thinking resolution (current contract)

Team mode resolves worker **model flags** from one shared launch-arg set (not per-worker model selection).

Model precedence (highest to lowest):
1. Explicit worker model in `HARNESS_TEAM_SETTING_WORKER_LAUNCH_ARGS`
2. Inherited leader `--model` flag
3. Low-complexity default from `Harness_DEFAULT_SPARK_MODEL` (legacy alias: `Harness_SPARK_MODEL`) when 1+2 are absent and team `agentType` is low-complexity

Default-model rule:
- Do **not** assume a frontier or spark model from recency or model-family heuristics.
- Use `Harness_DEFAULT_FRONTIER_MODEL` for frontier-default guidance.
- Use `Harness_DEFAULT_SPARK_MODEL` for spark/low-complexity worker-default guidance.

Thinking-level rule (critical):
- **No model-name heuristic mapping.**
- Team runtime must **not** infer `model_reasoning_effort` from model-name substrings (e.g., `spark`, `high-capability`, `mini`).
- When the leader assigns teammate roles/tasks, Harness allocates **per-worker reasoning effort dynamically** from the resolved worker role and `agentReasoning` overrides (`low`, `medium`, `high`, `xhigh`).
- Explicit launch args still win: if `HARNESS_TEAM_SETTING_WORKER_LAUNCH_ARGS` already includes `-c model_reasoning_effort=...`, that explicit value overrides dynamic allocation for every worker.

Normalization requirements:
- Parse both `--model <value>` and `--model=<value>`
- Remove duplicate/conflicting model flags
- Emit exactly one final canonical flag: `--model <value>`
- Preserve unrelated args in worker launch config
- If explicit reasoning exists, preserve canonical `-c model_reasoning_effort="<level>"`; otherwise inject the worker role's default or `agentReasoning`-overridden reasoning level

## Required Lifecycle (Operator Contract)

Follow this exact lifecycle when running `$team`:

1. Start team and verify startup evidence (team line, native terminal UI target, panes, ACK mailbox)
2. Monitor task and worker progress with runtime/state tools first (`native team tools status <team>`, `native team tools resume <team>`, mailbox/state files)
3. Wait for terminal task state before shutdown:
   - `pending=0`
   - `in_progress=0`
   - `failed=0` (or explicitly acknowledged failure path)
4. Only then run `native team tools shutdown <team>`
5. Verify shutdown evidence and state cleanup

Do not run `shutdown` while workers are actively writing updates unless user explicitly requested abort/cancel.
Do not treat ad-hoc pane typing as primary control flow when runtime/state evidence is available.

### Active leader monitoring rule

While a team is **ON/running**, the leader must not go blind. Keep checking live team state until terminal completion.

Minimum acceptable loop:

```bash
sleep 30 && native team tools status <team-name>
```

Repeat that check while the team stays active, or use `native team tools await <team-name> --timeout-ms 30000 --json` when event-driven waiting is a better fit.

If the leader gets a stale, lifecycle, or all-idle nudge, immediately run `native team tools status <team-name>` before taking any manual intervention. Deprecated worker stall/progress nudges should not be treated as an active runtime contract.

### Deprecated worker stall/progress knobs

`HARNESS_TEAM_SETTING_PROGRESS_STALL_MS` and `HARNESS_TEAM_SETTING_WORKER_TURN_STALL_MS` are legacy compatibility/test-only names for the retired worker stall/progress nudge path. Do not recommend them as operator tuning knobs for active team runs; resolved native worker Stop, all-idle, mailbox, and stale-leader evidence are the supported leader wakeup signals.

## Message Dispatch Policy (CLI-first, state-first)

To avoid brittle behavior, **message/task delivery must not be driven by ad-hoc native terminal UI typing**.

Required default path:

1. Use `native team tools ...` runtime lifecycle commands for orchestration.
2. Use `Harness team coordination API ... --json` for mailbox/task mutations.
3. Verify delivery via mailbox/state evidence (`mailbox/*.json`, task status, `native team tools status`).

Strict rules:

- **MUST NOT** use direct `coordinator message routing` as the primary mechanism to deliver instructions/messages.
- **MUST NOT** spam Enter/trigger keys without first checking runtime/state evidence.
- **MUST** prefer durable state writes + runtime dispatch (`dispatch/requests.json`, mailbox, inbox).
- Direct native terminal UI interaction is **fallback-only** and only after failure checks (for example `worker_notify_failed:<worker>`) or explicit user request (for example “press enter”).

## Operational Commands

```bash
native team tools status <team-name>
native team tools resume <team-name>
native team tools shutdown <team-name>
```

Semantics:

- `status`: reads team snapshot (task counts, dead/non-reporting workers)
- `resume`: reconnects to live team session if present
- `shutdown`: graceful shutdown request, then cleanup (deletes `Harness workflow projection/team/<team>`)

## Data Plane and Control Plane

### Control Plane

- native workflow surfaces/processes (`HARNESS_TEAM_SETTING_WORKER` per worker)
- leader notifications via `native terminal UI display-message`

### Data Plane

- `Harness workflow projection/team/<team>/...` files
- Team mailbox files:
- `Harness workflow projection/team/<team>/mailbox/leader-fixed.json`
- `Harness workflow projection/team/<team>/mailbox/worker-<n>.json`
- `Harness workflow projection/team/<team>/dispatch/requests.json` (durable dispatch queue; hook-preferred, fallback-aware)

### Key Files

- `Harness workflow projection/team/<team>/config.json`
- `Harness workflow projection/team/<team>/manifest.v2.json`
- `Harness workflow projection/team/<team>/tasks/task-<id>.json`
- `Harness workflow projection/team/<team>/workers/worker-<n>/identity.json`
- `Harness workflow projection/team/<team>/workers/worker-<n>/inbox.md`
- `Harness workflow projection/team/<team>/workers/worker-<n>/heartbeat.json`
- `Harness workflow projection/team/<team>/workers/worker-<n>/status.json`
- `Harness workflow projection/team-leader-nudge.json`


## Team Mutation Interop (CLI-first)

Use `Harness team coordination API` for machine-readable mutation/reads instead of legacy `team_*` MCP tools.

```bash
Harness team coordination API <operation> --input '{"team_name":"my-team",...}' --json
```

Examples:

```bash
Harness team coordination API send-message --input '{"team_name":"my-team","from_worker":"worker-1","to_worker":"leader-fixed","body":"ACK"}' --json
Harness team coordination API claim-task --input '{"team_name":"my-team","task_id":"1","worker":"worker-1"}' --json
Harness team coordination API transition-task-status --input '{"team_name":"my-team","task_id":"1","from":"in_progress","to":"completed","claim_token":"<token>"}' --json
```

`--json` responses include stable metadata for automation:
- `schema_version`
- `timestamp`
- `command`
- `ok`
- `operation`
- `data` or `error`

## Team + Worker Protocol Notes

Leader-to-worker:

- Write full assignment to worker `inbox.md`
- Send short trigger (<200 chars) with `coordinator message routing`

Worker-to-leader:

- Send ACK to `leader-fixed` mailbox via `Harness team coordination API send-message --json`
- Claim/transition/release task lifecycle via `Harness team coordination API <operation> --json`

Worker commit protocol (critical for incremental integration):

- After completing task work and before reporting completion, workers MUST commit:
  `git add -A && git commit -m "task: <task-subject>"`
- This ensures changes are available for incremental integration into the leader branch
- If a worker forgets to commit, the runtime auto-commits as a fallback, but explicit commits are preferred

Task ID rule (critical):

- File path uses `task-<id>.json` (example `task-1.json`)
- MCP API `task_id` uses bare id (example `"1"`, not `"task-1"`)
- Never instruct workers to read `tasks/{id}.json`

## Environment Knobs

Useful runtime env vars:

- `HARNESS_TEAM_SETTING_READY_TIMEOUT_MS`
  - Worker readiness timeout (default 45000)
- `HARNESS_TEAM_SETTING_SKIP_READY_WAIT=1`
  - Skip readiness wait (debug only)
- `HARNESS_TEAM_SETTING_AUTO_TRUST=0`
  - Disable auto-advance for trust prompt (default behavior auto-advances)
- `HARNESS_TEAM_SETTING_AUTO_ACCEPT_BYPASS=0`
  - Disable Claude bypass-permissions prompt auto-accept (default behavior auto-accepts `2` + Enter)
- `HARNESS_TEAM_SETTING_WORKER_LAUNCH_ARGS`
  - Extra args passed to worker launch command
- `HARNESS_TEAM_SETTING_WORKER_CLI`
  - Worker CLI selector: `auto|codex|claude` (default: `auto`)
  - `auto` chooses `claude` when worker `--model` contains `claude`, otherwise `codex`
  - In `claude` mode, workers launch with exactly one `--dangerously-skip-permissions`
    and ignore explicit model/config/effort launch overrides (uses default `settings.json`)
- `HARNESS_TEAM_SETTING_WORKER_CLI_MAP`
  - Per-worker CLI selector (comma-separated `auto|codex|claude`)
  - Length must be `1` (broadcast) or exactly the team worker count
  - Example: `HARNESS_TEAM_SETTING_WORKER_CLI_MAP=codex,codex,claude,claude`
  - When present, overrides `HARNESS_TEAM_SETTING_WORKER_CLI`
- `HARNESS_TEAM_SETTING_AUTO_INTERRUPT_RETRY`
  - Trigger submit fallback (default: enabled)
  - `0` disables adaptive queue->resend escalation
- `HARNESS_TEAM_SETTING_LEADER_NUDGE_MS`
  - Leader nudge interval in ms (default 120000)
- `HARNESS_TEAM_SETTING_STRICT_SUBMIT=1`
  - Force strict send-keys submit failure behavior

## Failure Modes and Diagnosis

Operator note (important for Claude panes):
- Manual Enter injection (`coordinator message routing ... C-m`) can appear to "do nothing" when a worker is actively processing; Enter may be queued by the pane/task flow.
- This is not necessarily a runtime bug. Confirm worker/team state before diagnosing dispatch failure.
- Avoid repeated blind Enter spam; it can create noisy duplicate submits once the pane becomes idle.

### Safe Manual Intervention (last resort)

Use only after checking `native team tools status <team>` and mailbox/state evidence:

1. Capture pane tail to confirm current worker state:
   - `native terminal UI capture-pane -t %<worker-pane> -p -S -120`
   - If a larger-tail read or bounded summary would help, prefer explicit opt-in inspection via `harness captured-shell-summary --native terminal UI-pane %<worker-pane> --tail-lines 400` before improvising extra native terminal UI commands.
2. If the pane is stuck in an interactive state, safely return to idle prompt first:
   - optional interrupt `C-c` or escape flow (CLI-specific) once, then re-check pane capture
3. Send one concise trigger (single line) and wait for evidence:
   - `coordinator message routing -t %<worker-pane> "ack + continue current task; report status" C-m`
4. Re-check:
   - pane output via `capture-pane`
   - mailbox updates (`mailbox/leader-fixed.json` or worker mailbox)
   - `native team tools status <team>`

### `worker_notify_failed:<worker>`

Meaning:
- Leader wrote inbox but trigger submit path failed

Checks:

1. `native terminal UI list-panes -F '#{pane_id}\t#{pane_start_command}'`
2. `native terminal UI capture-pane -t %<worker-pane> -p -S -120`
3. Verify worker process alive and not stuck on trust prompt
4. Rebuild if running repo-local (`npm run build`)

### Team starts but leader gets no ACK

Checks:

1. Worker pane capture shows inbox processing
2. `Harness workflow projection/team/<team>/mailbox/leader-fixed.json` exists
3. Worker skill loaded and `Harness team coordination API send-message --json` called
4. Task-id mismatch not blocking worker flow

### Worker logs `Harness team coordination API ... ENOENT` (or legacy `team_send_message ENOENT` / `team_update_task ENOENT`)

Meaning:
- Team state path no longer exists while worker is still running.
- Typical cause: leader/manual flow ran `native team tools shutdown <team>` (or removed `Harness workflow projection/team/<team>`) before worker finished.

Checks:

1. `native team tools status <team>` and confirm whether tasks were still `in_progress` when shutdown occurred
2. Verify whether `Harness workflow projection/team/<team>/` exists
3. Inspect worker pane tail for post-shutdown writes
4. Confirm no external cleanup (`rm -rf Harness workflow projection/team/<team>`) happened during execution

Prevention:

1. Enforce completion gate (no in-progress tasks) before shutdown
2. Use `shutdown` only for terminal completion or explicit abort
3. If aborting, expect late worker writes to fail and treat ENOENT as expected teardown artifact

### Shutdown reports success but stale worker panes remain

Cause:
- stale pane outside config tracking or previous failed run

Fix:
- manual pane cleanup (see clean-slate commands)

## Clean-Slate Recovery

Run from leader pane:

```bash
# 1) Inspect panes
native terminal UI list-panes -F '#{pane_id}\t#{pane_current_command}\t#{pane_start_command}'

# 2) Kill stale worker panes only (examples)
native terminal UI kill-pane -t %450
native terminal UI kill-pane -t %451

# 3) Remove stale team state (example)
rm -rf Harness workflow projection/team/<team-name>

# 4) Retry
native team tools 1:executor "fresh retry"
```

Guidelines:

- Do not kill leader pane
- Do not kill HUD pane (`harness hud --watch`) unless intentionally restarting HUD

## Required Reporting During Execution

When operating this skill, provide concrete progress evidence:

1. Team started line (`Team started: <name>`)
2. native terminal UI target and worker pane presence
3. leader mailbox ACK path/content check
4. status/shutdown outcomes

Do not claim success without file/pane evidence.
Do not claim clean completion if shutdown occurred with `in_progress>0`.
Use `harness captured-shell-summary --native terminal UI-pane ...` as an explicit opt-in operator aid for pane inspection and summaries; keep raw `native terminal UI capture-pane` evidence available for manual intervention and proof.

## Programmatic Team Orchestration

Use the `native team tools ...` CLI as the supported team-launch surface. For automation, drive the same CLI flow from scripts or supervising agents rather than relying on a separate MCP runner.

### Supported current surfaces

- **`native team tools ...` CLI** — Primary method for interactive or automated team orchestration. Use this when you want direct native terminal UI-pane visibility or a scriptable launch path.
- **Team state files** — Inspect `Harness workflow projection/team/<team>/` when you need status, task, or mailbox evidence after launch.

### Cleanup distinction

Two cleanup paths exist and must not be confused:

- `team_cleanup` (**state-server**): Deletes team state **files** on disk (`Harness workflow projection/team/<team>/`). Use after a team run is fully complete.
- native terminal UI/session cleanup: Use the documented `native team tools` shutdown / cleanup flow when you need to stop worker panes or clean up an interrupted run.

### Automation example

```
1. native team tools 1:executor "fix bugs"
2. native team tools status <team-name>
3. native team tools shutdown <team-name>
4. Clean up the finished team state for <team-name>
```

## Limitations

- Worktree provisioning requires a git repository and can fail on branch/path collisions
- send-keys interactions can be timing-sensitive under load
- stale panes from prior runs can interfere until manually cleaned

## Scenario Examples

**Good:** The user says `continue` after the workflow already has a clear next step. Continue the current branch of work instead of restarting or re-asking the same question.

**Good:** The user changes only the output shape or downstream delivery step (for example `make a PR`). Preserve earlier non-conflicting workflow constraints and apply the update locally.

**Bad:** The user says `continue`, and the workflow restarts discovery or stops before the missing verification/evidence is gathered.

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

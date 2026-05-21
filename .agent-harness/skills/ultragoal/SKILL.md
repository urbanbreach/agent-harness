---
name: ultragoal
description: Create and execute durable repo-native multi-goal plans over external goal context artifacts.
---

# Ultragoal Workflow

Use when the user asks for `ultragoal`, `create-goals`, `complete-goals`, durable multi-goal planning, or sequential execution over Codex `/goal`.

## Purpose

`ultragoal` turns a brief into repo-native artifacts and then drives a Codex goal safely through goal tools. New plans default to an aggregate Codex goal for the whole ultragoal run while Harness tracks G001/G002 story progress in the ledger.

- `Harness goal ledger artifacts/brief.md`
- `Harness goal ledger artifacts/goals.json`
- `Harness goal ledger artifacts/ledger.jsonl`

## Create goals

1. Run one of:
   - `harness workflow goal-ledger create-goals --brief "<brief>"`
   - `harness workflow goal-ledger create-goals --brief-file <path>`
   - `cat <brief> | harness workflow goal-ledger create-goals --from-stdin`
   - `harness workflow goal-ledger create-goals --codex-goal-mode per-story --brief "<brief>"` only when one fresh Codex thread per story is explicitly preferred
2. Inspect `Harness goal ledger artifacts/goals.json` and refine if needed.

## Complete goals

Loop until `harness workflow goal-ledger status` reports all goals complete:

1. Run `harness workflow goal-ledger complete-goals`.
2. Read the printed handoff.
3. Call `get_goal`.
4. If no active Codex goal exists, call `create_goal` with the printed payload. In aggregate mode, if the same aggregate Codex objective is already active, continue the current Harness story without creating a new Codex goal.
5. Complete the current Harness story only.
6. Run a completion audit against the story objective and real artifacts/tests.
7. In aggregate mode, do **not** call `update_goal` for intermediate stories; checkpoint with a fresh `get_goal` snapshot whose aggregate objective is still `active`. On the final story only, first run the mandatory final cleanup/review gate below; call `update_goal({status: "complete"})` only after that gate is clean, then call `get_goal` again for a fresh `complete` snapshot.
8. Checkpoint the durable ledger with that snapshot. Intermediate aggregate checkpoints use only `--codex-goal-json`; final clean checkpoints also require `--quality-gate-json`:
   `harness workflow goal-ledger checkpoint --goal-id <id> --status complete --evidence "<evidence>" --codex-goal-json <get_goal-json-or-path> [--quality-gate-json <quality-gate-json-or-path>]`
9. If blocked or failed, checkpoint failure:
   `harness workflow goal-ledger checkpoint --goal-id <id> --status failed --evidence "<blocker/evidence>"`
10. For legacy per-story completed-goal blockers, preserve the non-terminal blocker with:
   `harness workflow goal-ledger checkpoint --goal-id <id> --status blocked --evidence "<completed legacy Codex goal blocks create_goal in this thread>" --codex-goal-json <get_goal-json-or-path>`
11. Resume failed goals with `harness workflow goal-ledger complete-goals --retry-failed`.

## Use Ultragoal and Team together

Use ultragoal and team together for a durable Ultragoal story that benefits from parallel execution. Ultragoal remains leader-owned: `Harness goal ledger artifacts/goals.json` stores the story plan and `Harness goal ledger artifacts/ledger.jsonl` stores checkpoints. Team is the parallel execution engine and returns task/evidence status to the leader.

The leader checkpoints Ultragoal from Team evidence with a fresh `get_goal` snapshot:

```sh
harness workflow goal-ledger checkpoint --goal-id <id> --status complete --evidence "<team evidence mentioning Harness goal ledger artifacts and <id>>" --codex-goal-json <fresh-get_goal-json-or-path>
```

Workers do not own ultragoal goal state, do not create worker ultragoal ledgers, and do not checkpoint Ultragoal. Team launch remains explicit; Ultragoal does not auto-launch Team and performs no hidden Codex goal mutation.

## Mandatory final cleanup and review gate

The final ultragoal story is not complete until the active agent has run the final quality gate:

1. Run targeted verification for the story.
2. Run `ai-slop-cleaner` on changed files only; if there are no relevant edits, the cleaner still runs and records a passed/no-op report.
3. Rerun verification after the cleaner pass.
4. Run `$code-review`. Clean means `codeReview.recommendation: "APPROVE"` and `codeReview.architectStatus: "CLEAR"`; `COMMENT`, `WATCH`, `REQUEST CHANGES`, and `BLOCK` are non-clean.
5. If review is non-clean, do **not** call `update_goal`. Record durable blocker work instead:

   ```sh
   harness workflow goal-ledger record-review-blockers --goal-id <id> --title "Resolve final code-review blockers" --objective "<blocker-resolution objective>" --evidence "<review findings>" --codex-goal-json <active-get-goal-json-or-path>
   ```

   This marks the current story `review_blocked`, appends a pending blocker-resolution story, keeps the Codex goal active, and lets `harness workflow goal-ledger complete-goals` start the blocker next. In legacy per-story mode, the blocker may need a fresh/available Codex goal context because the old per-story Codex goal remains active/incomplete.

6. If review is clean, call `update_goal({status: "complete"})`, call `get_goal`, and checkpoint with a structured final gate:

   ```sh
   harness workflow goal-ledger checkpoint --goal-id <id> --status complete --evidence "<tests/files/review evidence>" --codex-goal-json <fresh-complete-get-goal-json-or-path> --quality-gate-json <quality-gate-json-or-path>
   ```

`--quality-gate-json` must include:

```json
{
  "aiSlopCleaner": { "status": "passed", "evidence": "cleaner report" },
  "verification": { "status": "passed", "commands": ["npm test"], "evidence": "post-cleaner verification" },
  "codeReview": { "recommendation": "APPROVE", "architectStatus": "CLEAR", "evidence": "final review synthesis" }
}
```

## Constraints

- The shell command cannot directly invoke Codex interactive `/goal`; it emits a model-facing handoff for the active Codex agent.
- Never call `create_goal` when `get_goal` reports a different active goal.
- Never call `update_goal` unless the aggregate run or legacy per-story goal is actually complete.
- In aggregate mode, intermediate story checkpoints require a matching `active` Codex snapshot; final story completion requires a matching `complete` snapshot after `update_goal`.
- Completion checkpoints require read-only Codex snapshot reconciliation: pass fresh `get_goal` JSON/path with `--codex-goal-json`; shell commands and hooks must not mutate Codex goal state.
- Treat `ledger.jsonl` as the durable audit trail; checkpoint after every success or failure.

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

## Use when

Use this skill when the matching `$` workflow command or catalog entry is selected and the operator request fits the workflow description.

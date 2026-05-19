---
name: autoresearch
description: Stateful validator-gated research loop with native-hook persistence
---

# Autoresearch

Autoresearch is the skill-first replacement for the deprecated `omx autoresearch` command.
It keeps the useful measured-research loop, but it now runs as a native-hook stateful workflow instead of a direct CLI or native terminal UI launch surface.

## Use when
- You want a Ralph-ish persistent research loop
- The task should keep nudging until explicit validation evidence exists
- You want init-time choice between script validation and prompt+architect validation

## Do not use when
- You want the old `omx autoresearch` command surface (hard-deprecated)
- You want detached native terminal UI or split-pane launch parity
- You have not decided the validation regime yet

## Core contract
1. **Init chooses validation mode.** Pick exactly one:
   - `mission-validator-script`
   - `prompt-architect-artifact`
2. **Persist mode state** in `Harness workflow projection/.../autoresearch-state.json` including:
   - `validation_mode`
   - `completion_artifact_path`
   - `mission_validator_command` **or** `validator_prompt`
   - optional `output_artifact_path`
3. **Completion is artifact-gated.** The loop does not stop because the model says “done”, because a stop hook fired once, or because several turns were no-ops.
4. **Direct CLI launch is gone.** Use `$deep-interview --autoresearch` for intake and `$autoresearch` for execution.

## Completion artifact contract

### `mission-validator-script`
The completion artifact must exist and record a passing validator result, for example:

```json
{
  "status": "passed",
  "passed": true,
  "summary": "metric improved beyond baseline"
}
```

### `prompt-architect-artifact`
The completion artifact must include both an architect approval verdict and an output artifact path, for example:

```json
{
  "validator_prompt": "Review the research output against the mission.",
  "architect_review": { "verdict": "approved" },
  "output_artifact_path": ".omx/specs/autoresearch-demo/report.md"
}
```

## Recommended flow
1. Run `$deep-interview --autoresearch` to clarify mission + evaluator.
2. Materialize `.omx/specs/autoresearch-{slug}/mission.md`, `sandbox.md`, and `result.json`.
3. Start `$autoresearch` with the chosen validation mode stored in mode state.
4. Let stop-hook / auto-nudge continue until the completion artifact satisfies the chosen validation mode.
5. Finish only after the validator artifact is complete.

## Migration note
- `omx autoresearch` is hard-deprecated.
- No direct CLI launch.
- No native terminal UI split-pane launch.
- No noop-count completion gate.

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

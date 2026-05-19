---
name: pipeline
description: Configurable pipeline orchestrator for sequencing stages
---

# Pipeline Skill

`$pipeline` is the configurable pipeline orchestrator for Harness. It sequences stages
through a uniform `PipelineStage` interface, with state persistence and resume support.

## Default Autopilot Pipeline

The canonical Harness pipeline sequences:

```
RALPLAN (consensus planning) -> team-exec (Codex CLI workers) -> ralph-verify (architect verification)
```

## Configuration

Pipeline parameters are configurable per run:

| Parameter | Default | Description |
|-----------|---------|-------------|
| `maxRalphIterations` | 10 | Ralph verification iteration ceiling |
| `workerCount` | 2 | Number of Codex CLI team workers |
| `agentType` | `executor` | Agent type for team workers |

## Stage Interface

Every stage implements the `PipelineStage` interface:

```typescript
interface PipelineStage {
  readonly name: string;
  run(ctx: StageContext): Promise<StageResult>;
  canSkip?(ctx: StageContext): boolean;
}
```

Stages receive a `StageContext` with accumulated artifacts from prior stages and
return a `StageResult` with status, artifacts, and duration.

## Built-in Stages

- **ralplan**: Consensus planning (planner + architect + critic). Skips only when both `prd-*.md` and `test-spec-*.md` planning artifacts already exist, and carries any `deep-interview-*.md` spec paths forward for traceability.
- **team-exec**: Team execution via Codex CLI workers. Always the Harness execution backend.
- **ralph-verify**: Ralph verification loop with configurable iteration count.

## State Management

Pipeline state persists via the ModeState system at `Harness workflow projection/pipeline-state.json`.
The HUD renders pipeline phase automatically. Resume is supported from the last incomplete stage.

- **On start**: `harness workflow evidence write --input '{"mode":"pipeline","active":true,"current_phase":"stage:ralplan"}' --json`
- **On stage transitions**: `harness workflow evidence write --input '{"mode":"pipeline","current_phase":"stage:<name>"}' --json`
- **On completion**: `harness workflow evidence write --input '{"mode":"pipeline","active":false,"current_phase":"complete"}' --json`

## API

```typescript
import {
  runPipeline,
  createAutopilotPipelineConfig,
  createRalplanStage,
  createTeamExecStage,
  createRalphVerifyStage,
} from './pipeline/index.js';

const config = createAutopilotPipelineConfig('build feature X', {
  stages: [
    createRalplanStage(),
    createTeamExecStage({ workerCount: 3, agentType: 'executor' }),
    createRalphVerifyStage({ maxIterations: 15 }),
  ],
});

const result = await runPipeline(config);
```

## Relationship to Other Modes

- **autopilot**: Autopilot can use pipeline as its execution engine (v0.8+)
- **team**: Pipeline delegates execution to team mode (Codex CLI workers)
- **ralph**: Pipeline delegates verification to ralph (configurable iterations)
- **ralplan**: Pipeline's first stage runs RALPLAN consensus planning

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

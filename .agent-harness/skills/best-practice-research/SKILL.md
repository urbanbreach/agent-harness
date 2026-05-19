---
name: best-practice-research
description: "[Harness] Bounded best-practice research wrapper using official/upstream evidence first"
argument-hint: "<technology|decision|practice question>"
---

# Best-Practice Research

Use this skill when a task depends on current external best practices, version-aware guidance, standards, official recommendations, or upstream behavior. This is a workflow wrapper: it routes evidence gathering and synthesis; it is not a new research authority and it does not replace `researcher`.

## Purpose

Produce a cited, reusable best-practice answer or handoff that separates current external evidence from repo-local facts and dependency-selection decisions.

## Activate When

- The user asks for best practices, recommended approach, current guidance, official recommendations, standards, or version-aware external behavior.
- `$ralplan`, `$deep-interview`, `$team`, or another workflow needs current external evidence before planning or execution can be correct.
- The task involves an already chosen technology and needs authoritative usage guidance, migration notes, API behavior, lifecycle rules, or current safety guidance.

## Do Not Activate When

- The answer is fully repo-local; use `explore` for codebase facts.
- The main question is whether to adopt, replace, upgrade, or compare dependencies; use `dependency-expert`.
- The user only needs implementation against already-grounded requirements; use `executor`, `$ralph`, or `$team` as appropriate.
- The task can be answered from stable local project conventions without current external lookup.

## Specialist Routing

1. Use `explore` first for brownfield facts: current code usage, local constraints, versions, config, and integration points.
2. Use `researcher` for official/upstream docs, release notes, standards, migration guides, source-backed examples, and current best-practice evidence for an already chosen technology.
3. Use `dependency-expert` only for adoption/upgrade/replacement/comparison decisions.
4. Return to the caller with explicit evidence, uncertainty, and any implementation handoff constraints.

## Source-Quality Rules

- Prefer official documentation, upstream source, release notes, changelogs, standards, and maintainer guidance.
- Include source URLs for material claims.
- State date/version context for current best-practice claims.
- Label third-party summaries as supplemental; do not use them before official/upstream sources.
- Flag stale, conflicting, undocumented, or version-mismatched evidence.
- Do not over-fetch: gather the smallest evidence set that can support the decision.

## Workflow

1. Classify the question: conceptual best practice, implementation guidance, migration/version guidance, standards/compliance guidance, or mixed local + external guidance.
2. Gather repo-local facts with `explore` when local usage or constraints affect the answer.
3. Gather external evidence with `researcher` when current or version-aware practice affects correctness.
4. Synthesize a concise answer with source quality, version/date context, caveats, and an implementation or planning handoff.
5. Stop when the answer is grounded enough for the caller; otherwise report the exact blocker or specialist handoff needed.

## Output Contract

```md
## Best-Practice Research: <question>

### Direct Recommendation
<actionable guidance or decision support>

### Evidence Used
- Official/upstream: <source URL> — <what it establishes>
- Supplemental, if any: <source URL> — <why it is secondary>

### Version / Date Context
<versions, dates, release channels, or unknowns>

### Repo-Local Context
<facts from explore, or "not needed">

### Boundaries / Non-goals
<what this research does not decide>

### Handoff
<planning/execution/test implications>
```

## Stop Rules

- Stop after a source-backed recommendation is reusable by the caller.
- Stop and route upward if the task becomes dependency comparison, broad architecture, or implementation.
- Do not continue researching when remaining work would only polish wording rather than change the recommendation.

Task: {{ARGUMENTS}}

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

---
name: ask
description: Ask a local external advisor CLI (Claude or Gemini) and capture a reusable artifact
---

# Ask (Local Advisor CLI)

Use a locally installed external advisor CLI for focused questions, reviews, brainstorming, or second opinions. This skill replaces the separate `ask-claude` and `ask-gemini` skills.

## Usage

```bash
$ask claude <question or task>
$ask gemini <question or task>
omx ask claude "<question or task>"
omx ask gemini "<question or task>"
```

## Backend selection

- Use `claude` when the user asks for Claude, Anthropic, or the previous `$ask-claude` behavior.
- Use `gemini` when the user asks for Gemini or the previous `$ask-gemini` behavior.
- If no backend is specified, choose the installed backend that best matches the user request; if neither is clearly available, explain that a local CLI is required.

## Local CLI commands

Claude:

```bash
omx ask claude "{{ARGUMENTS}}"
claude -p "{{ARGUMENTS}}"
```

Gemini:

```bash
omx ask gemini "{{ARGUMENTS}}"
gemini -p "{{ARGUMENTS}}"
```

If needed, adapt to the user's installed CLI variant while keeping local execution as the default path. Do not silently switch to an MCP or remote provider when the local binary is missing.

## Artifact requirement

After local execution, save a markdown artifact to:

```text
.omx/artifacts/ask-<backend>-<slug>-<timestamp>.md
```

Minimum artifact sections:
1. Original user task
2. Backend and final prompt sent to the CLI
3. Raw CLI output
4. Concise summary
5. Action items / next steps

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

## Purpose

Provide a native Harness workflow protocol for this skill so command dispatch, state projection, evidence, and closeout remain coordinator-owned and replayable.

## Use when

Use this skill when the matching `$` workflow command or catalog entry is selected and the operator request fits the workflow description.

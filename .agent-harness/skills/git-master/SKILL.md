---
name: git-master
description: Git workflow guidance for clean history, safe diffs, and evidence-rich commits in the harness workspace.
---

# Git master

Use this skill when preparing commits, reviewing local changes, or reasoning about branch state.

## Workflow
- Inspect `git status --short` before editing or committing.
- Treat uncommitted changes you did not make as user-owned; work around them.
- Keep commits focused on one behavioral purpose.
- Prefer non-interactive git commands.
- Avoid destructive commands unless the user explicitly requested them.

## Commit notes
- Use the Lore commit style for this repository.
- The first line should explain why the change exists.
- Include useful trailers such as `Constraint:`, `Rejected:`, `Confidence:`, `Scope-risk:`, `Tested:`, and `Not-tested:`.
- Mention exact verification commands that were run.

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

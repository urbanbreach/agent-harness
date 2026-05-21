---
name: git-master
description: Git expert for atomic commits, rebasing, and history management
---

# Git Master Command

Routes to the git-master agent for git operations.

## Purpose

Provide the legacy runtime git-master routing contract while recording Harness workflow state and evidence through coordinator-owned events.

## Use when

Use this skill for git operations that need atomic commits, rebasing, branch hygiene, history inspection, or style detection from repository history.

## Usage

```
/git-master <git task>
```

## Routing

```text
Use /prompts:git-master with the user task.
```

## Capabilities
- Atomic commits with conventional format
- Interactive rebasing
- Branch management
- History cleanup
- Style detection from repo history

## Harness substrate override

When this skill is loaded by `agent-harness`, route the user task through the native `task` tool using the `git-master` subagent profile instead of `/prompts:git-master` text routing.

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

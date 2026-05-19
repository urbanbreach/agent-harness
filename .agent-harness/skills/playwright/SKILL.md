---
name: playwright
description: Browser automation guidance for Playwright-backed MCP sessions, screenshots, and interaction evidence.
tools: [webfetch, read, bash]
commands:
  - npx playwright --version
  - npx @playwright/mcp --help
environment:
  allow:
    - PLAYWRIGHT_*
    - BROWSER_*
mcp:
  playwright:
    command: npx @playwright/mcp
permissions:
  webfetch: ask
  bash: ask
---

# Playwright browser automation

Use this skill when a task needs browser inspection, interaction, screenshots, accessibility checks, or console/network evidence.

## Operating notes
- Prefer a skill-scoped Playwright MCP server when available; do not assume it is globally running.
- Treat browser dependencies as optional: report missing `npx`, Playwright, or browser binaries as actionable setup diagnostics.
- Persist screenshots, traces, console summaries, and network findings as task evidence when the lane is enabled.
- Keep live/browser work environment-gated and avoid credentials unless explicitly provided for the task.

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

---
name: rust-best-practices
description: Baseline Rust guidance for the harness workspace, with emphasis on invariants, focused diffs, and verification.
---

# Rust best practices

Use this starter skill when you need a lightweight reminder of how this repository expects Rust changes to land.

## Core expectations
- Preserve coordinator/runtime invariants in `harness-core`; keep UI-specific logic out of core state transitions.
- Prefer small, reviewable diffs and extraction over redesign.
- Reuse existing helpers and patterns before adding abstractions.
- Keep errors contextual and actionable.
- Run the narrowest verification that proves the change, then summarize the evidence.

## Repository-specific reminders
- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Add targeted crate tests for the touched behavior.
- For TUI work, include `cargo test -p harness-tui`.
- For PTY/live helper changes, keep deterministic env guards intact and use the documented live-proxy order.

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

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
- For TUI work, include `cargo nextest run -p harness-tui`.
- For PTY/live helper changes, keep deterministic env guards intact and use the documented live-proxy order.

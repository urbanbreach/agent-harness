---
name: rust-best-practices
description: Rust implementation and verification checklist for harness changes.
---

Prefer small, typed changes.

Checklist:
- preserve existing behavior unless the task explicitly changes it
- keep public APIs and event/runtime invariants stable
- run cargo fmt, cargo check, cargo clippy, and relevant tests
- report changed files, verification, and remaining risks

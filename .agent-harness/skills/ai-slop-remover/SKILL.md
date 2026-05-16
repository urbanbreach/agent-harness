---
name: ai-slop-remover
description: Cleanup guidance for removing vague, duplicated, over-engineered, or AI-generated slop while preserving behavior.
tools: [read, grep, edit, bash]
commands:
  - cargo fmt --all -- --check
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
permissions:
  read: allow
  grep: allow
  edit: ask
  bash: ask
---

# AI slop remover

Use this skill for cleanup/refactor passes focused on clarity without behavior drift.

## Cleanup rules
- Lock behavior with targeted tests before editing when coverage is missing.
- Prefer deletion, consolidation, and existing helpers over new abstractions.
- Remove speculative comments, duplicated branches, placeholder code, and vague naming.
- Keep the diff small and rerun the narrowest verification that proves behavior stayed intact.

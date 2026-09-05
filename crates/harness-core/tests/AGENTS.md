# CORE TEST GUIDE

## OVERVIEW

Integration and contract coverage for core runtime behavior. Score 9: 170 Rust files (+3), 13 immediate subdirectories (+2), 100% code bytes (+2), and more than 30 structurally counted declarations (+2); this is a distinct fixture domain.

## STRUCTURE

```text
tests/
├── coord/                           # numbered coordinator scenarios
├── common/                          # stores, providers, tools, event waiters
├── conversation_projection/        # canonical context and compaction protocol
├── resume_plan/                     # recovery, watermarks, resumability
├── session_lineage_materialization/ # child history publication
├── integrations_matrix/             # plugin/MCP/ACP isolation
├── foreign_session/                 # discovery and replay-only import
└── context_budget/                   # exact and unknown-limit accounting
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Coordinator regression | `coord_test.rs`, `coord/*.rs` | Aggregator includes numbered scenarios |
| Shared runtime fixture | `common/coord_fixtures.rs` | Setup, fake clocks, event helpers |
| Scripted providers/tools | `common/coord_fixtures/` | Capturing, blocking, delayed, budget doubles |
| Compaction V2 | `coord/27_*` through `coord/31_*` | Budget, pressure, protocol, denial, isolation |
| Replay/projection | `conversation_projection/`, `resume_plan/`, `transcript_projection/` | Semantic reconstruction |
| Persistence boundary | `foreign_session/`, `session_lineage_materialization/` | Temporary JSONL/artifact trees |
| Performance contract | `perf/` | Override with `HARNESS_PERF_RESUME_PLAN_BUDGET_MS` |

## CONVENTIONS

- Use arrange/act/assert and identify events by durable IDs, correlation, sequence, owner, and terminal state.
- Subscribe to the exact event/state signal before triggering async work, then await a bounded timeout.
- Prefer `FakeClock`, scripted providers, injected executors, temporary directories, and `UnwrapOrAbort`.
- Exercise close/reopen/replay when durability is under test; use real local boundaries where relevant.
- Preserve fixture wiring: `coord_test.rs` includes shared fixtures and numbered bodies into isolated modules.
- Snapshot machine-consumed envelope shapes; compatibility tests may carry reasoned deprecated allowances.

## ANTI-PATTERNS

- Do not use fixed sleeps, polling delays, or immediate assertions for asynchronous coordinator behavior.
- Do not mock away the event store, provider barrier, permission gate, or replay boundary being asserted.
- Do not execute historical tools/hooks in resume fixtures or mutate source histories during import tests.
- Do not accept duplicate terminals, orphan tool results, sequence gaps, or incomplete turns as success.
- Do not update snapshots to hide schema drift; inspect the durable contract change first.
- Do not infer exact capacity in unknown-limit fixtures or hard-code a hidden model window.
- Do not add prose-copy tests; assert parsed fields, sentinels, ordering, and shipped contract data.

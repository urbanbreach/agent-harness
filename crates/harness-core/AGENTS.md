# AGENTS: crates/harness-core

## OVERVIEW
Core runtime crate: event schema, coordinator, scheduling, permissions, config, projections, transcript state, hashline edits, redaction, and deterministic storage.

Read the workspace root `AGENTS.md` first for search scope, commands, and cross-crate invariants.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Coordinator runtime | `src/coord.rs`, `src/coord/` | Single scheduling authority; provider loop, permissions, hooks, staleness, compaction. |
| Event schema | `src/event.rs` | `EventEnvelopeV1`, payload variants, actor/correlation/causation metadata. |
| Event stores | `src/store.rs` | In-memory + JSONL persistence; append-only sequencing expectations. |
| Permissions | `src/perm.rs` | Capability → permission-kind mapping and policy resolution. |
| Tool contracts | `src/tool.rs` | Tool traits, capabilities, canonical ids, artifacts. |
| Config | `src/config.rs`, `src/config/` | Discovery, validation, public schema shape, compatibility inputs. |
| Projections/replay state | `src/proj.rs`, `src/transcript_projection.rs` | Pure derived state for replay/UI/resume/export/debugging surfaces. |
| Agent/session metadata | `src/agent.rs`, `src/session_lineage.rs`, `src/session_title.rs`, `src/session_paths.rs` | Runtime identity, lineage, titles, and storage layout. |
| Hashline edits | `src/edit/hashline.rs` | Anchor hashing, overlap rejection, atomic apply semantics. |
| Scheduler | `src/sched.rs` | Concurrency keys, slots, progress/staleness snapshots. |

## INVARIANTS
- Coordinator owns all event appends, task scheduling, permission resolution, hooks, and run/agent lifecycle transitions.
- Events are immutable and append-only; replay rebuilds state from `seq`-ordered JSONL without side effects.
- Late task results after cancellation become `TaskResultLate`; discard side effects.
- Slot gates are coordinator-managed counters; avoid semaphore-in-`select!` cancellation footguns.
- Permission `ask` pauses for `ResolvePermission`; headless `ask` denies unless a scenario explicitly resolves it.
- Worker actors must not spawn agents directly; supervisor-only spawn violations emit policy violations.
- Compaction writes artifacts/events and preserves recent turns according to config/model metadata; it must not rewrite event logs.
- Redact secrets before persistence and artifact summaries.

## CONFIG CONTRACT
- Canonical runtime keys: `provider`, `model`, `small_model`, `agent`, `default_agent`, `permission`, `mcp`, `skills`, `instructions`.
- Canonical permission names: `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp`.
- Legacy aliases/shapes are migration inputs only; new examples/docs/tests should use the harness-centered split.
- Unsupported top-level areas fail validation explicitly.

## TESTS
```bash
cargo test -p harness-core
cargo test -p harness-core --test coord
cargo test -p harness-core --test coord_auth
cargo test -p harness-core --test mcp_config
cargo test -p harness-core --test permission_policy_supports_native_tool_permission_kinds
cargo test -p harness-core --test replay_preserves_batch_and_child_task_metadata_for_native_and_compat_paths
cargo test -p harness-core --test transcript_projection
```

Run root drift checks when event/config public contracts change:
```bash
cargo test -p harness --test event_docs_reference
cargo test -p harness --test config_docs_reference
```

## ANTI-PATTERNS
- Do not bypass permission checks by executing tools directly from agents/UI/tests.
- Do not mutate stored events or rely on non-contiguous sequence numbers.
- Do not move UI-specific state or rendering decisions into core projections.
- Do not hardcode config paths; use loader discovery/precedence helpers.
- Do not add event variants without updating architecture docs and drift tests.

# AGENTS: crates/harness-core

## OVERVIEW
Core runtime crate: event schema, coordinator, scheduling, permissions, config, projections, transcript state, hashline edits, redaction, team orchestration, and deterministic storage.

Read the workspace root `AGENTS.md` first for search scope, cross-crate invariants, and command lanes.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Coordinator runtime | `src/coord.rs`, `src/coord/` | Single scheduling authority; provider loop, permissions, hooks, staleness, compaction, teams. |
| Event schema | `src/event.rs` | `EventEnvelopeV1`, payload variants, actor/correlation/causation metadata. |
| Event stores | `src/store.rs` | JSONL persistence, append sequencing, writer-lock recovery. |
| Permissions | `src/perm.rs` | Capability to permission-kind mapping and policy resolution. |
| Tool contracts | `src/tool.rs` | Tool traits, capabilities, canonical ids, artifact store. |
| Config | `src/config.rs`, `src/config/` | Discovery, validation, public schema shape, compatibility inputs. |
| Projections | `src/proj.rs`, `src/transcript_projection.rs` | Pure replay/UI/resume/export/debugging views. |
| Agent/session metadata | `src/agent.rs`, `src/session_lineage.rs`, `src/session_title.rs`, `src/session_paths.rs` | Runtime identity, lineage, titles, storage layout. |
| Hashline edits | `src/edit/hashline.rs` | Anchor hashing, overlap rejection, atomic apply. |

## STRUCTURE
```text
src/coord/       # hooks, provider_context, team, tool_execution, focused tests
src/config/      # discovery, loader, public config surface
src/edit/        # hashline patch engine
src/snapshots/   # insta snapshots for event envelopes
```

## INVARIANTS
- Coordinator owns all event appends, task scheduling, permission resolution, hooks, and run/agent lifecycle transitions.
- Events are immutable and append-only; replay rebuilds state from contiguous `seq`-ordered JSONL without side effects.
- Late task results after cancellation become `TaskResultLate`; do not apply side effects after cancellation wins.
- Permission `ask` pauses for `ResolvePermission`; headless `ask` denies unless a scenario explicitly resolves it.
- Worker actors must not spawn agents directly; supervisor-only violations emit policy violations.
- Compaction writes artifacts/events and preserves recent turns from config/model metadata; it must not rewrite event logs.
- Redact secrets before persistence and artifact summaries.

## CONFIG CONTRACT
- Canonical runtime keys include `provider`, `model`, `small_model`, `agent`, `default_agent`, `permission`, `mcp`, `skills`, `instructions`.
- Canonical permission names are `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp`.
- Legacy aliases/shapes are migration inputs only; examples/docs/tests should use the harness-centered split.
- Unsupported top-level areas fail validation explicitly.

## TESTS
```bash
cargo test -p harness-core
cargo test -p harness-core --test coord_test
cargo test -p harness-core --test mcp_config_test
cargo test -p harness-core --test permission_policy_supports_native_tool_permission_kinds_test
cargo test -p harness-core --test transcript_projection_test
```
Run root drift checks when event/config public contracts change:
```bash
cargo test -p harness --test event_docs_reference_test
cargo test -p harness --test config_docs_reference_test
```

## ANTI-PATTERNS
- Do not bypass permission checks by executing tools directly from agents, UI, or tests.
- Do not mutate stored events or rely on non-contiguous sequence numbers.
- Do not move UI-specific state or rendering decisions into core projections.
- Do not hardcode config paths; use loader discovery and workspace/session path helpers.
- Do not add event variants without updating architecture docs and drift tests.

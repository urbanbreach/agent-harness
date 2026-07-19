# AGENTS: crates/harness-core

## OVERVIEW
Core runtime crate: event schema, coordinator, scheduling, permissions, config, projections, transcript state, hashline edits, redaction, team orchestration, session lineage, and deterministic storage.

Read root `AGENTS.md` first for search scope, cross-crate invariants, and command lanes.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Coordinator runtime | `src/coord.rs`, `src/coord/`, `src/coord/AGENTS.md` | Single scheduling authority; provider loop, permissions, hooks, staleness, compaction, questions, teams. |
| Agent/provider context | `src/agent.rs`, `src/agent/`, `src/provider_args.rs`, `src/conversation.rs` | Provider-facing message shaping, streaming state, sanitized historical tool metadata. |
| Event schema | `src/event.rs`, `src/event/` | `EventEnvelopeV1`, payload variants, builders, team events, actor/correlation/causation metadata. |
| Event stores | `src/store.rs`, `src/store/` | JSONL persistence, append sequencing, writer-lock recovery. |
| Permissions | `src/perm.rs`, `src/coord/permission.rs` | Capability-to-permission-kind mapping and policy resolution. |
| Tool contracts | `src/tool.rs` | Tool traits, capabilities, canonical ids, artifact store. |
| Config | `src/config.rs`, `src/config/`, `src/config/AGENTS.md` | Discovery, validation, public schema shape, compatibility inputs, model/provider registries. |
| Auth/model resolution | `src/auth/`, `src/model_resolution.rs`, `src/config/model_*` | Stored auth providers, catalog metadata, variants, capability inference. |
| Projections | `src/proj.rs`, `src/proj/`, `src/transcript_projection.rs`, `src/transcript_projection/` | Pure replay/UI/resume/export/debugging views. |
| Agent/session metadata | `src/agent_catalog.rs`, `src/session_lineage.rs`, `src/session_lineage/`, `src/session_title.rs`, `src/session_paths.rs` | Runtime identity, profile catalog, lineage, titles, storage layout. |
| Workspaces/paths | `src/workspace.rs`, `src/path_selector.rs`, `src/path_display.rs` | Workspace roots, display paths, path selection helpers. |
| Hashline edits | `src/edit/hashline.rs` | Anchor hashing, overlap rejection, atomic apply. |
| Extension descriptors | `src/extension_manifest.rs` | Descriptor-only V1 manifest parser, schema, replay metadata. |
| Integration lifecycle | `src/integrations/` | Plugin install/activate (package-entry load + receipt; no .so/wasm) /deactivate/remove + ACP connection state machine. |

## INVARIANTS
- Coordinator owns all event appends, task scheduling, permission resolution, hooks, compaction, and run/agent lifecycle transitions.
- `coord.rs` is still large by design; extract only focused helpers that keep the coordinator as the visible single authority.
- Events are immutable and append-only; replay rebuilds state from contiguous `seq`-ordered JSONL without side effects.
- Late task results after cancellation become `TaskResultLate`; do not apply side effects after cancellation wins.
- Permission `ask` pauses for `ResolvePermission`; headless `ask` denies unless a scenario explicitly resolves it.
- Worker actors must not spawn agents directly; supervisor-only violations emit policy violations.
- Compaction writes artifacts/events and preserves recent turns from config/model metadata; it must not rewrite event logs.
- Conversation/projection code must stay replay-derived and must not call providers, tools, hooks, MCP, network, or the CLI.
- Redact secrets before persistence, summaries, support exports, and artifact indexes.
- Typed extension manifests are descriptor-only in V1: no runtime tool registration, command execution, MCP launch, provider decorators, external code loading, or session mutation.

## CONFIG CONTRACT
- Canonical runtime keys include `provider`, `model`, `small_model`, `model_profile`, `agent`, `default_agent`, `permission`, `mcp`, `skills`, `instructions`, `enabled_providers`, `disabled_providers`.
- Canonical permission names are `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp`.
- Legacy aliases/shapes are migration inputs only; examples/docs/tests should use the harness-centered split.
- Runtime config belongs to `harness.json{,c}`; TUI config belongs to `tui.json{,c}`.
- Unsupported top-level areas fail validation explicitly.

## TESTS
```bash
cargo nextest run -p harness-core
cargo nextest run -p harness-core --test coord_test
cargo nextest run -p harness-core --test coord_auth_test
cargo nextest run -p harness-core --test coord_ast_grep_auth_test
cargo nextest run -p harness-core --test extension_manifest_test
cargo nextest run -p harness-core --test integrations_lifecycle_test
cargo nextest run -p harness-core --test conversation_projection_test
cargo nextest run -p harness-core --test mcp_config_test
cargo nextest run -p harness-core --test model_variant_resolution_test
cargo nextest run -p harness-core --test native_metadata_replay_test
cargo nextest run -p harness-core --test permission_policy_supports_native_tool_permission_kinds_test
cargo nextest run -p harness-core --test resume_plan_test
cargo nextest run -p harness-core --test session_lineage_materialization_test
cargo nextest run -p harness-core --test transcript_projection_test
```

Run root drift checks when event/config public contracts change:
```bash
cargo nextest run -p harness --test event_docs_reference_test
cargo nextest run -p harness --test config_docs_reference_test
cargo nextest run -p harness --test config_schema_cli_test
```

## ANTI-PATTERNS
- Do not bypass permission checks by executing tools directly from agents, UI, providers, or tests.
- Do not mutate stored events or rely on non-contiguous sequence numbers.
- Do not move UI-specific state or rendering decisions into core projections.
- Do not hardcode config paths; use loader discovery and workspace/session path helpers.
- Do not add event variants without updating architecture/session docs and drift tests.
- Do not turn descriptor-only extension manifests into a plugin host without a new coordinator-owned runtime design.

# AGENTS: crates/harness-core

## OVERVIEW
Core runtime crate: event schema, coordinator, scheduling, permissions, config, projections, memory, worktrees, sandbox, OAuth, cron, team orchestration, session lineage, and deterministic storage.

Read root `AGENTS.md` first for search scope, cross-crate invariants, and command lanes. Deep scopes: `src/coord/AGENTS.md`, `src/config/AGENTS.md`, `tests/AGENTS.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Coordinator runtime | `src/coord.rs`, `src/coord/`, `src/coord/AGENTS.md` | `spawn_coordinator`, `Command`, `CoordinatorHandle`, `RunState`; single scheduling authority; provider loop, permissions, hooks, staleness, compaction, questions, teams. |
| Agent/provider context | `src/agent.rs`, `src/agent/`, `src/provider_args.rs`, `src/conversation.rs` | Provider-facing message shaping, streaming state, sanitized historical tool metadata. |
| Event schema | `src/event.rs`, `src/event/` | `EventEnvelopeV1`/`EventV1` payload variants, builders, team events, actor/correlation/causation metadata. |
| Event stores | `src/store.rs`, `src/store/` | JSONL persistence, append sequencing, writer-lock recovery. |
| Permissions | `src/perm.rs`, `src/perm/`, `src/coord/permission.rs` | Capability-to-permission-kind mapping and policy resolution. |
| Tool contracts | `src/tool.rs` | Tool traits, capabilities, canonical ids, artifact store. |
| Config | `src/config.rs`, `src/config/`, `src/config/AGENTS.md` | Public contract, settings registry/write, discovery, validation, model/provider registries. |
| Auth/OAuth | `src/auth/`, `src/browser_oidc*.rs`, `src/mcp_oauth*.rs`, `src/sleep_wake_auth.rs`, `src/model_resolution.rs`, `src/config/model_*` | Stored auth, OAuth flows, sleep/wake parity, catalog metadata, variants, capability inference. |
| Projections | `src/proj.rs`, `src/proj/`, `src/transcript_projection.rs`, `src/transcript_projection/` | Pure replay/UI/resume/export/debugging views. |
| Memory/prompt queue | `src/memory.rs`, `src/memory/`, `src/prompt_queue.rs`, `src/prompt_rewind.rs` | Durable scoped memory, prompt queue persistence/ordering/drain, compaction parity. |
| Worktrees/VCS | `src/worktree.rs`, `src/cow_worktree.rs`, `src/vcs.rs`, `src/jujutsu.rs` | Worktree snapshots, VCS integration and trust. |
| Sandbox/trust | `src/sandbox.rs`, `src/sandbox/`, `src/folder_trust.rs` | Network/worktree confinement, folder trust. |
| Cron/scheduler | `src/cron_schedule.rs`, `src/cron_execute.rs`, `src/sched.rs`, `src/scheduler_leaf.rs` | Recurring schedules, task scheduling, concurrency keys. |
| Team/session infra | `src/team_registry.rs`, `src/team_mailbox_journal.rs`, `src/session_lineage.rs`, `src/session_lineage/`, `src/foreign_session.rs`, `src/session_leaf.rs`, `src/workspace_hub.rs` | Team routing, lineage, foreign-session import, session/workspace leaves. |
| Agent/session metadata | `src/agent_catalog.rs`, `src/session_title.rs`, `src/session_paths.rs` | Runtime identity, profile catalog, titles, storage layout. |
| Workspaces/paths | `src/workspace.rs`, `src/path_selector.rs`, `src/path_display.rs` | Workspace roots, display paths, path selection helpers. |
| Hashline edits | `src/edit/hashline.rs`, `src/edit_attribution.rs` | Anchor hashing, overlap rejection, atomic apply, attribution. |
| Extension descriptors | `src/extension_manifest.rs`, `src/extension_registry.rs` | Descriptor-only V1 manifest parser, schema, replay metadata, registry. |
| Integration lifecycle | `src/integrations/`, `src/integration_leaf.rs` | Plugin install/activate (package-entry load + receipt; no .so/wasm) /deactivate/remove + ACP connection state machine. |
| Attachment transport | `src/attachment_transport/` | Attachment checkpointing/redaction for provider transport. |
| Test owners | `tests/AGENTS.md` | Owner boundaries: coord fan-in, fixtures, replay/projection, permission/auth, integrations, perf. |

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
- Canonical runtime keys include `provider`, `model`, `small_model`, `model_profile`, singleton `agent`, `permission`, `mcp`, `skills`, `instructions`, `enabled_providers`, `disabled_providers`.
- Canonical permission names are `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp`.
- Legacy aliases/shapes are migration inputs only; examples/docs/tests should use the harness-centered split.
- Runtime config belongs to `harness.json{,c}`; TUI config belongs to `tui.json{,c}`.
- Unsupported top-level areas fail validation explicitly.

## TESTS
```bash
cargo nextest run -p harness-core
```
Owner test boundaries (coord fan-in, fixtures, replay/projection, permission/auth, integrations, perf): see `tests/AGENTS.md`.

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

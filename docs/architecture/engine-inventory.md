# Engine inventory

This inventory freezes the `060ee1fd` starting point for the engine simplification.
It distinguishes observed source structure from the future target; it is not a release claim.

## Measured contract

`scripts/engine-metrics.sh` emits `engine-metrics-v1`. It reads first-party production Rust under
`crates/*/src`, strips `cfg(test)` modules, and excludes Rust test paths. It records the supplied
baseline separately from a fresh measurement and sets a drift flag when those values differ.

| Baseline fact | Supplied value |
|---|---:|
| production LOC | 205939 |
| harness-core LOC | 54964 |
| harness-tui LOC | 100800 |
| session bucket | 14207 |
| projection bucket | 5944 |
| compaction bucket | 1585 |
| coordinator bucket | 15121 |
| EventV1 variants | 39 |
| active session-compaction variants | 2 |
| durable reducers | 5 |
| SIZE_OK all/reachable | 192/185 |

The emitted JSON also contains current and baseline frozen-overlap LOC, per-crate LOC, every
overlap path with content hash, event/compaction variants, reducer paths, and mock-runtime
measurements. Timing fields are volatile; structural fields are deterministic.

## Frozen overlap file set

The frozen overlap set is every production Rust file whose path or declared module contains
`session`, `conversation`, `transcript`, `projection`, `provider_context`, or `compaction`.
The metrics artifact records its sorted paths, individual SHA-256 hashes, and a path-list SHA-256;
that artifact is the authoritative exhaustive list for the pinned commit.

## Interactive TUI flow

```text
user prompt → crates/harness/src/tui.rs::execute → harness-tui runtime/AppState session admission
→ crates/harness-core/src/coord.rs::spawn_coordinator
→ coord/agent_turn_phases.rs::prepare_provider_transform_phase (context construction)
→ config/model_selection.rs::resolve_model_selection (model resolution)
→ agent/provider_boundary.rs::build_provider_context_messages (provider request)
→ agent/streaming.rs::stream_assistant_response_once (streaming)
→ coord/agent_turn_phases.rs::append_assistant_message_end_phase (assistant commit)
→ coord/agent_turn_phases.rs::execute_agent_tool_phase (tool calls)
→ coord/agent_turn_phases.rs::append_tool_result_message_phase (tool results)
→ coord/command_loop.rs (continuation) → store.rs::JsonlFileEventStore::append (persistence)
→ coord/provider_context/restore.rs::restore_provider_context_from_history +
  proj/resume_projection.rs::project_resume_plan + TUI AppState::SessionProjection::ingest_event
  (resume/replay)
```

## Headless flow

```text
user prompt → crates/harness/src/lib.rs::execute_cli → crates/harness/src/run.rs::execute_with_io
→ crates/harness-core/src/coord.rs::spawn_coordinator
→ coord/agent_turn_phases.rs::prepare_provider_transform_phase (prompt admission/context)
→ config/model_selection.rs::resolve_model_selection (model resolution)
→ agent/provider_boundary.rs::build_provider_context_messages (provider request)
→ agent/streaming.rs::stream_assistant_response_once (streaming)
→ coord/agent_turn_phases.rs::append_assistant_message_end_phase (assistant commit)
→ coord/agent_turn_phases.rs::execute_agent_tool_phase (tool calls)
→ coord/agent_turn_phases.rs::append_tool_result_message_phase (tool results)
→ coord/command_loop.rs (continuation) → store.rs::JsonlFileEventStore::append (persistence)
→ coord/provider_context/restore.rs::restore_provider_context_from_history +
  proj/resume_projection.rs::project_resume_plan + crates/harness/src/replay.rs::summarize_session
  (resume/replay)
```

Both routes use the same coordinator and event store. `harness run --scenario golden_path
--deterministic` is real offline runtime evidence for the headless route; no live-provider or PTY
claim is inferred from it.

## Phase 0 backend subsystem matrix

`I`/`H` mean normal interactive/headless reachability. `Unit` and `integration/PTY` name the
owner surface when present; `none recorded` is deliberate, not an implied pass.

| Subsystem | Owning files/modules; runtime entry | I/H | Unit; integration/PTY | Runtime evidence | Status; disposition |
|---|---|---|---|---|---|
| CLI/TUI bootstrap | `harness/src/{lib.rs,tui.rs}`; `execute_cli`, `tui::execute` | yes/yes | CLI/TUI owners; PTY owner | headless golden; TUI none recorded | canonical; Keep |
| configuration discovery and merging | `harness-core/src/config/{discovery.rs,loader.rs}`; bootstrap config load | yes/yes | config owners; no PTY | golden config bootstrap | canonical; Keep |
| provider registry | `harness-core/src/{provider_catalog.rs,provider_protocol.rs}` | yes/yes | provider catalog owners; no PTY | mock provider in golden | canonical; Keep |
| model registry and model resolution | `config/{model_limits.rs,model_limit_resolution.rs,model_catalog.rs,model_selection.rs}` | yes/yes | model-limit resolution, catalog poisoning, CLI catalog, and TUI metadata owners | `harness models list` | canonical limits/provenance; Keep |
| model variants | `config/model_limit_resolution.rs`, `config/model_selection.rs` | yes/yes | model variant and model-limit resolution owners | configured variant catalog | explicit-field override semantics; Keep |
| context-window resolution | `config/model_limits.rs`; legacy M03 consumer in `coord/compaction_support.rs` | yes/yes | model-limit owners; compaction owners in M03 | CLI/TUI resolved metadata | canonical source established; budget migration remains Move |
| provider request construction | `agent/provider_boundary.rs::build_provider_context_messages`, providers request modules | yes/yes | provider boundary owners; no PTY | golden mock request | canonical boundary; Keep |
| prompt/system-context construction | `dynamic_prompt.rs`, `agent/provider_boundary.rs` | yes/yes | prompt owners; no PTY | golden mock request | duplicated context assembly; Consolidate |
| session persistence | `store.rs::JsonlFileEventStore::append` | yes/yes | store owners; no PTY | golden `events.jsonl` | canonical journal; Keep |
| session listing | `harness/src/{sessions.rs,replay.rs::inspect_session_catalog}` | yes/yes | replay/session owners; no PTY | corpus metric unavailable | linear read model; Move |
| session continuation | `run.rs`, `coord/handle.rs::resume_run` | yes/yes | resume owners; TUI replay owner | none recorded | supported; Consolidate |
| replay | `harness/src/replay.rs::summarize_session` | yes/yes | replay owners; no PTY | golden log is inspectable | canonical read-only boundary; Keep |
| conversation projection | `conversation.rs::project_conversation` | yes/yes | conversation projection owners; no PTY | replay path | duplicated reducer; Consolidate |
| transcript projection | `transcript_projection.rs::project_transcript` | yes/yes | transcript owners; TUI PTY owner | replay path | duplicated reducer; Consolidate |
| TUI session projection | `harness-tui/src/app/session_projection.rs::SessionProjection::ingest_event` | yes/no | TUI owner; PTY owner | none recorded | duplicated reducer; Move |
| prompt queue | `harness/src/prompt_queue_cmd.rs`, core prompt queue modules | yes/yes | command owners; no PTY | none recorded | supported; Keep |
| tool execution | `coord/tool_execution.rs`, `coord/handle.rs::execute_agent_tool_call` | yes/yes | tool/coord owners; PTY owner | golden tool batch | canonical authority; Keep |
| permissions | `perm.rs`, `coord/permission.rs` | yes/yes | permission owners; PTY owner | golden permission path | canonical authority; Keep |
| subagents and child sessions | `coord/child_session.rs`, `session_lineage.rs` | yes/yes | lineage/coord owners; no PTY | none recorded | supported; Consolidate |
| background tasks | `coord/{task_lifecycle.rs,background_notifications.rs}`, `proj/background_projection.rs` | yes/yes | background owners; no PTY | none recorded | duplicated views; Consolidate |
| compaction | `coord/session_compaction.rs`, `coord/compaction/` | yes/yes | compaction owners; no PTY | none recorded | overlapping active/legacy; Consolidate |
| provider-context checkpoints | `coord/provider_context/restore.rs` reads legacy checkpoints; writer helper has no callers | legacy-only/legacy-only | checkpoint restore owners; no PTY | none recorded | legacy read path; Disable writer helper then Delete |
| operational memory | active `coord/{session_compaction.rs,compaction/file_ops.rs}`; orphan `coord/provider_context/operational_memory.rs` is uncompiled | yes/yes | `operational_memory_context_tests`; no PTY | no separate runtime capture | active behavior Keep; dead duplicate Delete |
| branching/fork/clone/rewind | `session_lineage/`, `prompt_rewind.rs`, CLI sessions | yes/yes | lineage owners; TUI navigation owner | none recorded | supported; Move |
| crash recovery | `crash_recovery.rs`, session reopen | yes/yes | recovery owners; no PTY | none recorded | supported; Keep |
| extension/hook paths | `integrations/`, `coord/hooks.rs` | yes/yes | integration/hook owners; no PTY | none recorded | incomplete boundary; Disable dynamic execution claims |
| legacy compatibility code | legacy event/checkpoint/replay readers | yes/yes | replay/session owners; TUI replay owner | none recorded | legacy; Disable mutation then Delete adapter after migration |

## M02 model-limit inventory receipt

The canonical type is `config::ResolvedModelLimits`; its three fields each carry typed provenance,
and `MaxInputSemantics::ProviderVisibleInputTokens` fixes the input meaning. Numeric window fields
were removed from model-family capability resolution and from resolved profile/catalog/TUI model
metadata. `RecordedRuntimeContext` persists the canonical record while retaining non-authoritative
numeric mirrors only for the pre-M03 compaction compatibility path. Request-budget calculations
and deletion of those mirrors remain assigned to M03.

## Current compaction and reducer inventory

`EventV1` has legacy `CompactionRequested`, `CompactionWritten`, and `CompactionApplied` variants,
plus the active session-compaction pair `SessionCompaction` and deprecated `CompactionFailed`.
`BranchSummary` is a separate branch-summary event, not a session-compaction variant. The five
durable reducer paths are conversation projection, transcript projection, resume projection,
provider-context restore fold, and TUI `SessionProjection`.

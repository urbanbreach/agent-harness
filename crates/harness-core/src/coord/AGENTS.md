# AGENTS: crates/harness-core/src/coord

## OVERVIEW
Coordinator implementation modules. This directory is the runtime's single scheduling, event-append, permission, hook, compaction, task, and lifecycle authority.

Read `../../AGENTS.md` first. Keep the coordinator visible as the authority; extract focused helpers only when they preserve that boundary.

## STRUCTURE
```text
coord.rs                  # Command enum, Coordinator, spawn_coordinator, top-level errors
coord/
├── handle.rs             # CoordinatorHandle: public command boundary for runtime callers
├── command_loop.rs        # Command dispatch into RunState handlers
├── state.rs               # RunState, TaskState, pending permissions, child turns
├── run_lifecycle.rs       # run start/finish/fail/stop metadata
├── agent_turn_runtime.rs  # scheduling and execution of agent turns
├── agent_turn_phases.rs   # provider/tool/assistant phase loop
├── agent_turn_completion.rs # turn finalization and compaction trigger
├── task_lifecycle.rs      # task spawn/finish/cancel/late-result handling
├── tool_execution.rs      # tool execution dispatch and result capture
├── provider_lifecycle.rs  # provider connect/retry/stream lifecycle
├── session_compaction.rs  # session compaction trigger and checkpointing
├── prompt_queue_drain.rs  # prompt queue drain scheduling
├── background_notifications.rs # background request projection wakeups
├── permission.rs, question.rs # permission and question resolution
├── hooks.rs               # lifecycle hook execution summaries
├── provider_context/      # compaction planning, restore, operational memory
├── compaction/            # compaction planning, cut points, summaries, tokens
├── compaction_support.rs  # compaction coordination helpers
├── tool_metadata.rs       # native tool metadata replay projection
├── event_helpers.rs       # append helpers; keep event writes centralized
├── child_session.rs       # forked/child session mirroring
├── snapshot.rs, revert.rs # workspace snapshot/revert events
├── formatter/             # formatter discovery/status
└── tests/                 # focused coordinator acceptance suites
```

## FLOW RULES
- New command handling starts at `Command` in `coord.rs`, dispatches in `command_loop.rs`, and mutates only `RunState`/owned helper state. `CoordinatorHandle` in `handle.rs` is the only public command boundary.
- Event appends go through `event_helpers.rs` or the existing lifecycle helper for that domain; do not write events from ad hoc helper code.
- Permission checks precede tool execution in `tool_execution.rs`; provider lifecycle transitions and retries stay in `provider_lifecycle.rs`.
- Session compaction, prompt queue drain, and provider-context planning are coordinator-owned paths; do not move them into tools or replay.
- Child session mirrors copy stable event prefixes/artifacts only through `child_session.rs`; do not let worker actors spawn agents directly.

## WHERE TO LOOK
| Task | Location | Tests |
|------|----------|-------|
| Command/handle dispatch | `handle.rs`, `command_loop.rs` | `cargo nextest run -p harness-core --test coord_test` |
| Turn loop | `agent_turn_runtime.rs`, `agent_turn_phases.rs`, `agent_turn_completion.rs` | `cargo nextest run -p harness-core --test coord_test` |
| Tool execution | `tool_execution.rs` | `cargo nextest run -p harness-core --test coord_test` |
| Provider lifecycle | `provider_lifecycle.rs` | `cargo nextest run -p harness-core --test coord_test` |
| Permissions/questions | `permission.rs`, `question.rs` | `cargo nextest run -p harness-core --test permission_policy_supports_native_tool_permission_kinds_test` |
| Task/background lifecycle | `task_lifecycle.rs`, `background_notifications.rs` | `cargo nextest run -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test` |
| Session compaction/context | `session_compaction.rs`, `compaction/`, `compaction_support.rs`, `provider_context/`, `agent_turn_completion.rs` | `cargo nextest run -p harness-core --test coord_test compaction` |
| Prompt queue drain | `prompt_queue_drain.rs` | `cargo nextest run -p harness-core --test memory_queue_compaction_test` |
| Replay metadata | `tool_metadata.rs`, `event_helpers.rs` | `cargo nextest run -p harness-core --test native_metadata_replay_test` |
| Snapshots/reverts | `snapshot.rs`, `revert.rs` | `cargo nextest run -p harness-core --test coord_test workspace` |

## ANTI-PATTERNS
- Do not bypass `RunState` to share mutable lifecycle state across modules.
- Do not move replay/projection logic into coordinator helpers.
- Do not append provider raw requests/responses, auth headers, cookies, keys, PEM blocks, or hidden reasoning text.
- Do not add command variants without event/docs/test coverage for the resulting observable behavior.
- Do not make formatter, hook, MCP, or tool helper failures silently mutate the event log outside coordinator-owned paths.

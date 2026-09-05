# HARNESS TOOLS TEST GUIDE

## OVERVIEW

Score 13: 26 direct integration-test files, seven scenario subdirectories, `common/mod.rs`, and measured high symbol/export density define a distinct coordinator-to-tool contract suite.

## STRUCTURE

```text
tests/
|- common/                              # shared workspaces, event waiters, fake MCP, policies
|- native_execution_surface/            # native IDs, schemas, permissions, edit compatibility
|- native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/ # agent scenarios
|- native_code_lsp/                      # LSP fixtures and semantic operation cases
|- native_question_tool/                 # question permission and compatibility cases
|- skill_load_discovery/                 # precedence and activation scenarios
`- support/                              # large-output dogfood support
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Reuse a test workspace or actor | `common/mod.rs`, `common/workspace.rs`, `common/tool_context.rs` | Prefer existing coordinator-backed fixtures. |
| Assert event completion | `common/event_log.rs`, `common/event_reader.rs` | Subscribe/wait for the exact terminal event. |
| Test native surface compatibility | `native_execution_surface_test.rs`, `native_execution_surface/` | Aggregator includes numbered scenario modules. |
| Test task/batch/background | `native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test.rs` | Preserve input order and coordinator permission checks. |
| Test skills | `skill_load_discovery_test.rs`, `skill_load_discovery/` | Cover precedence, malformed entries, denial, and activation caps. |
| Test LSP/edit routing | `native_code_lsp_test.rs`, `native_workspace_edit_routing_test.rs` | Use fake servers and temporary workspaces. |

## CONVENTIONS

- Use descriptive snake-case test names and explicit `// arrange`, `// act`, `// assert` markers.
- Drive tools through coordinator registries with scripted transports, fake servers, temporary roots, and exact JSON/event assertions.
- Aggregator test targets use nested modules or `include!`; place a new scenario beside its owning target instead of creating an unrelated harness.
- Preserve deterministic ordering, seeded IDs, redacted metadata, and bounded output/artifact assertions.
- Update provider payload snapshots only through the explicit `UPDATE_PROVIDER_TOOL_PAYLOAD_SNAPSHOTS` path.

## ANTI-PATTERNS

- Never use fixed sleeps or timing luck; install the event/state waiter before triggering asynchronous work and keep waits bounded.
- Do not depend on live providers, network, real subprocesses, PTY, or native visual facilities in deterministic integration targets.
- Do not mock away coordinator permission, lineage, event append, or tool registration behavior that the test claims to cover.
- Do not expose raw tool arguments, loaded skill bodies, credentials, or inline reasoning in expected durable output.
- Do not nest batch, reorder parallel results, or allow worker redelegation in fixtures that model production constraints.
- Treat workspace-wide symbol reference centrality as unmeasured.

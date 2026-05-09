# AGENTS: crates/harness-tools

## OVERVIEW
Native tool registry and implementations for filesystem discovery/editing, shell execution, delegation/control-plane tools, network/code search, GitHub, LSP, and MCP integration.

Read the workspace root `AGENTS.md` first; runtime policy lives in `harness-core`, while this crate owns argument validation, execution, stable schemas, and tool-surface parity.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Registry composition | `src/lib.rs` | `coordinator_registry*`, `worker_registry*`, MCP/editing feature wiring. |
| Native wrappers | `src/native_tools.rs` | User-facing tool ids, aliases, blocked-command recovery hints. |
| Filesystem search | `src/fs_glob.rs`, `src/fs_grep.rs`, `src/fs_ls.rs`, `src/fs_walk.rs` | Workspace-safe discovery; limits and output modes. |
| Workspace edits | `src/workspace_edit.rs`, `src/hashline_*` | Read/write/edit routing, hashline anchors, apply artifacts. |
| Bash/network/GitHub | `src/lib.rs`, `src/network.rs`, `src/github.rs`, `src/http_client.rs` | Bash allowlist, network search/fetch, GitHub wrappers. |
| Delegation/control plane | `src/agent_ops.rs`, `src/control_plane.rs` | `task`, `background_output`, `batch`, `question`, `skill`, todos. |
| LSP | `src/code_lsp.rs`, `src/code_lsp_rename.rs`, `src/lsp_support.rs` | Diagnostics/symbols/references/rename; graceful unsupported responses. |
| MCP | `src/mcp.rs` | Config-backed server registration and generic call surfaces. |

## TOOL SURFACE RULES
- Keep canonical native ids stable: `read`, `list`, `glob`, `grep`, `edit`, `bash`, `task`, `background_output`, `batch`,
  `question`, `skill`, `webfetch`, `websearch`, `codesearch`, `lsp`.
- Coordinator registry may expose supervisor-only tools; worker registry must be filtered through `ActorKind::Worker`.
- Tool schemas use typed args and `deny_unknown_fields`; keep generated provider schemas strict and parity-tested.
- Use workspace-relative path resolution helpers; reject traversal/out-of-workspace access.
- `read` emits hashline anchors by default when hashline editing is enabled; edits should consume anchored views.
- Bash is for execution, not file IO/search/editing; blocked-command messages should point users to native tools.
- LSP/MCP availability is optional; fail with actionable structured errors rather than panics or silent fallbacks.
- MCP server tools are config-backed; keep generic `mcp.<server>.tool.call` discovery-oriented flows working.

## TESTS
```bash
cargo test -p harness-tools
cargo test -p harness-tools --test native_tool_parity_matrix
cargo test -p harness-tools --test native_execution_surface
cargo test -p harness-tools --test native_workspace_edit_routing
cargo test -p harness-tools --test native_control_plane_tools
cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order
cargo test -p harness-tools --test native_code_lsp
cargo test -p harness-tools --test native_code_search
cargo test -p harness-tools --test native_github
cargo test -p harness-tools --test mcp_generic
```

## ANTI-PATTERNS
- Do not duplicate permission policy here; enforce what `harness-core` decides.
- Do not add a tool without schema/parity coverage and a permission/capability story.
- Do not use ad hoc path joins for workspace files.
- Do not collapse full tool output into event summaries only; preserve artifact flow for large outputs.
- Do not make read/search tools mutate state.
- Do not assume GitHub/web/LSP/MCP calls are available in deterministic/offline lanes.

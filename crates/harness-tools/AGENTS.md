# AGENTS: crates/harness-tools

## OVERVIEW
Native tool registry and implementations for filesystem discovery/editing, shell execution, delegation/control-plane tools, network/code search, GitHub, LSP, MCP, session inspection, and team coordination.

Read root `AGENTS.md` first. Runtime policy lives in `harness-core`; this crate owns argument validation, execution, stable schemas, and tool-surface parity.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Registry composition | `src/lib.rs` | Coordinator/worker registry builders, MCP/editing feature wiring. |
| Native wrappers | `src/native_tools.rs` | User-facing tool ids, aliases, blocked-command recovery hints. |
| Filesystem search | `src/fs_glob.rs`, `src/fs_grep.rs`, `src/fs_ls.rs`, `src/fs_walk.rs` | Workspace-safe discovery, limits, output modes. |
| Workspace edits | `src/workspace_edit.rs`, `src/hashline_*` | Read/write/edit routing, hashline anchors, apply artifacts. |
| Bash/network/GitHub | `src/shell_run.rs`, `src/shell_safety.rs`, `src/network.rs`, `src/github.rs` | Shell allowlist, web fetch/search/code search, GitHub wrappers. |
| Delegation/control plane | `src/agent_ops.rs`, `src/control_plane.rs` | `task`, `background_output`, `batch`, `question`, `skill`, todos. |
| LSP | `src/code_lsp.rs`, `src/code_lsp_rename.rs`, `src/lsp_support.rs` | Diagnostics/symbols/references/rename; unsupported responses stay structured. |
| MCP | `src/mcp.rs` | Config-backed server registration and generic call surface. |
| Session/team tools | `src/session_tools.rs`, `src/team_ops.rs` | Replay-derived session reads and event-sourced team projections. |

## TOOL SURFACE RULES
- Keep canonical native ids stable: `read`, `list`, `glob`, `grep`, `edit`, `bash`, `task`, `background_output`, `batch`, `question`, `skill`, `webfetch`, `websearch`, `codesearch`, `lsp`.
- Coordinator registry may expose supervisor-only tools; worker registry must be filtered through `ActorKind::Worker`.
- Tool schemas use typed args and strict provider schemas; keep parity tests green when adding fields.
- Use workspace path helpers; reject traversal and out-of-workspace access.
- `read` emits hashline anchors by default when hashline editing is enabled; edits should consume anchored views.
- Bash is for execution, not file IO/search/editing; blocked-command messages should point users to native tools.
- LSP/MCP/network availability is optional in deterministic lanes; return actionable structured errors.
- `session_*` tools are replay-derived and must not run providers, hooks, network, MCP, or the CLI.

## TESTS
```bash
cargo test -p harness-tools
cargo test -p harness-tools --test native_tool_parity_matrix_test
cargo test -p harness-tools --test native_execution_surface_test
cargo test -p harness-tools --test native_workspace_edit_routing_test
cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test
cargo test -p harness-tools --test native_code_lsp_test
cargo test -p harness-tools --test skill_load_discovery_test
```

## ANTI-PATTERNS
- Do not duplicate permission policy here; enforce the decision `harness-core` made.
- Do not add a tool without schema/parity coverage and a permission/capability story.
- Do not use ad hoc path joins for workspace files.
- Do not collapse full output into event summaries only; preserve artifact flow for large outputs.
- Do not make read/search/session tools mutate state.

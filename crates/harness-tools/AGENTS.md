# AGENTS: crates/harness-tools

## OVERVIEW
Native tool registry and implementations for filesystem discovery/reading/editing, shell execution, delegation/control-plane tools, network/code search, GitHub, AST-grep, LSP, MCP, session inspection, and team coordination.

Read root `AGENTS.md` first. Runtime policy lives in `harness-core`; this crate owns argument validation, execution, stable schemas, artifacts, and tool-surface parity.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Registry composition | `src/lib.rs`, `src/tool_catalog.rs` | Coordinator/worker registry builders, catalog metadata, MCP/editing feature wiring. |
| Native wrappers/tree | `src/native_tools.rs`, `src/native_tools/` | User-facing tool ids, aliases, tree/schema helpers, blocked-command recovery hints. |
| Filesystem discovery/read | `src/fs_glob.rs`, `src/fs_grep.rs`, `src/fs_ls.rs`, `src/fs_read.rs`, `src/fs_walk.rs`, `src/read_window.rs` | Workspace-safe discovery, limits, output modes. |
| Workspace edits | `src/workspace_edit.rs`, `src/workspace_paths.rs`, `src/hashline_*` | Read/write/edit routing, hashline anchors, apply artifacts. |
| AST/code/LSP | `src/ast_grep.rs`, `src/ast_grep/`, `src/code_lsp.rs`, `src/code_lsp_rename.rs`, `src/code_lsp_rename/`, `src/lsp_support/` | Structural search/replace plus diagnostics/symbols/references/rename. |
| Bash/network/GitHub | `src/shell_run.rs`, `src/shell_safety.rs`, `src/network.rs`, `src/network/`, `src/github.rs`, `src/http_client.rs` | Shell allowlist, web fetch/search/code search, GitHub wrappers. |
| Delegation/control plane | `src/agent_ops.rs`, `src/agent_ops/`, `src/control_plane.rs`, `src/plan.rs`, `src/question_env.rs` | `task`, `background_output`, `batch`, `question`, `skill`, todos, child metadata. |
| Skill catalog | `src/skill_catalog.rs`, `src/skill_catalog/` | Project/global skill discovery, frontmatter/resources, compact catalog metadata. |
| MCP | `src/mcp.rs`, `src/mcp_render.rs`, `src/mcp_session.rs` | Config-backed server registration, rendering, and generic call surface. |
| Session tools | `src/session_tools.rs`, `src/session_tools/` | Replay-derived session reads and capped/redacted summaries. |
| Shared helpers | `src/env_vars.rs`, `src/limit_summary.rs`, `src/text.rs` | Environment access, truncation summaries, text formatting used across tools. |

## TOOL SURFACE RULES
- Keep canonical native ids stable: `read`, `list`, `glob`, `grep`, `edit`, `bash`, `task`, `background_output`, `batch`, `question`, `skill`, `webfetch`, `websearch`, `codesearch`, `lsp`.
- Coordinator registry may expose supervisor-only tools; worker registry must be filtered through `ActorKind::Worker`.
- Tool schemas use typed args and strict provider schemas; keep parity tests green when adding fields.
- Use workspace path helpers; reject traversal and out-of-workspace access.
- `read` emits hashline anchors by default when hashline editing is enabled; edits should consume anchored views.
- Bash is for execution, not file IO/search/editing; blocked-command messages should point users to native tools.
- AST-grep replace must stay structural and previewable; do not fall back to regex-shaped rewrites through this surface.
- `ast_grep_replace` maps to `edit`, defaults to dry-run, rejects truncated apply, validates current bytes, and writes diff artifacts.
- LSP/MCP/network availability is optional in deterministic lanes; return actionable structured errors.
- `session_*` tools are replay-derived and must not run providers, hooks, network, MCP, or the CLI.
- Team tools are event-sourced primitives; do not claim full Team Mode worktrees/tmux/mailbox unless runtime/tests ship it.
- Large outputs need artifact flow plus concise summaries; do not drop the full artifact when summarizing.

## TESTS
```bash
cargo nextest run -p harness-tools
cargo nextest run -p harness-tools --test native_tool_parity_matrix_test
cargo nextest run -p harness-tools --test native_execution_surface_test
cargo nextest run -p harness-tools --test native_workspace_edit_routing_test
cargo nextest run -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test
cargo nextest run -p harness-tools --test native_ast_grep_search_test
cargo nextest run -p harness-tools --test native_ast_grep_replace_test
cargo nextest run -p harness-tools --test native_code_lsp_test
cargo nextest run -p harness-tools --test native_control_plane_tools_test
cargo nextest run -p harness-tools --test native_question_tool_test
cargo nextest run -p harness-tools --test native_web_fetch_test
cargo nextest run -p harness-tools --test native_web_search_test
cargo nextest run -p harness-tools --test native_github_test
cargo nextest run -p harness-tools --test native_code_search_test
cargo nextest run -p harness-tools --test hashline_apply_test
cargo nextest run -p harness-tools --test mcp_generic_test
cargo nextest run -p harness-tools --test skill_load_discovery_test
cargo nextest run -p harness-tools --test session_info_tool_test
```

## ANTI-PATTERNS
- Do not duplicate permission policy here; enforce the decision `harness-core` made.
- Do not add a tool without schema/parity coverage and a permission/capability story.
- Do not use ad hoc path joins for workspace files.
- Do not collapse full output into event summaries only; preserve artifact flow for large outputs.
- Do not make read/search/session tools mutate state.
- Do not advertise low-level compatibility helpers as public tool ids without catalog/docs/parity coverage.

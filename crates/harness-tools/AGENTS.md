# AGENTS: crates/harness-tools

## OVERVIEW
Native tool registry and implementations for filesystem discovery/reading/editing, shell execution, delegation/control-plane tools, network/code search, GitHub, AST-grep, LSP, MCP, and session inspection.

Read root `AGENTS.md` first. Runtime policy lives in `harness-core`; this crate owns argument validation, execution, stable schemas, artifacts, and tool-surface contracts.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Registry/catalog | `src/lib.rs`, `src/tool_catalog.rs` | `coordinator_registry*`/`worker_registry*` builders, executor wiring, and `NativeToolCatalogEntry` metadata. |
| Tool wrappers/schemas | `src/native_tools.rs`, `src/native_tools/args.rs` | Public tool ids, typed arg schemas, aliases, blocked-command recovery hints. |
| Read | `src/fs_read.rs` (`args`/`render`/`window`), `src/read_window.rs` | Windowed reads, hashline anchor rendering, truncation, media sampling. |
| List | `src/native_tools/tree.rs`, `src/fs_walk.rs` | Recursive directory tree, ignore patterns, walk normalization helpers. |
| Glob/grep | `src/fs_glob.rs`, `src/fs_grep.rs` | Workspace-safe pattern search with limits and context. |
| Edits | `src/hashline_edit.rs`, `src/hashline_apply.rs`, `src/hashline_scan.rs`, `src/exact_edit.rs`, `src/exact_edit_match/`, `src/file_write.rs`, `src/apply_patch_tool.rs` (`matching`/`parser`/`plan`) | Hashline/exact/write/apply_patch execution, formatter and LSP-diagnostics post-edits. |
| Edit routing/paths | `src/workspace_edit.rs`, `src/workspace_paths.rs` | Read/write/edit routing, workspace path safety, traversal rejection. |
| Shell | `src/shell_run.rs`, `src/shell_run/sandbox_helper.rs`, `src/bin/harness-sandbox-helper.rs` | Execution, preview limits, Linux Landlock sandbox helper. |
| Shell safety | `src/shell_safety.rs` (`path_validation`) | Allowlist scanning, blocked-command guidance. |
| Delegation/control plane | `src/agent_ops.rs` (`background`/`batch`/`child_metadata`), `src/control_plane.rs`, `src/question_env.rs` | `task`, `background_output`, `background_cancel`, `batch`, `question`, `skill`, `todoread`/`todowrite`, `invalid`. |
| Skill catalog | `src/skill_catalog.rs` (`frontmatter`/`resources`) | Project/global skill discovery, compact catalog metadata. |
| AST-grep | `src/ast_grep.rs` (`adapter`) | Structural search/replace; replace maps to `edit`, dry-run, diff artifacts. |
| LSP | `src/code_lsp.rs`, `src/code_lsp_rename.rs` (`preview`/`text`), `src/lsp_support.rs` (`session`) | Diagnostics/symbols/references, rename previews, LSP sessions. |
| Network/GitHub | `src/network.rs` (`remote_search`), `src/github.rs`, `src/http_client.rs` | Web fetch/search/code search, GitHub issue/PR wrappers. |
| MCP | `src/mcp.rs`, `src/mcp_render.rs`, `src/mcp_session.rs` | Config-backed server registration, rendering, generic call surface. |
| Session inspection | `src/session_tools.rs` (`summaries`) | Replay-derived session reads, capped/redacted summaries. |
| Shared helpers | `src/arg_parse.rs`, `src/env_vars.rs`, `src/limit_summary.rs`, `src/text.rs` | Tool arg parsing, environment access, truncation summaries, text formatting. |

## TOOL SURFACE RULES
- Keep canonical native ids stable: `read`, `list`, `glob`, `grep`, `edit`, `bash`, `task`, `background_output`, `batch`, `question`, `skill`, `webfetch`, `websearch`, `codesearch`, `lsp`.
- Coordinator registry may expose supervisor-only tools; worker registry must be filtered through `ActorKind::Worker`.
- Tool schemas use typed args and strict provider schemas; keep owner tests green when adding fields.
- Use workspace path helpers; reject traversal and out-of-workspace access.
- `read` emits hashline anchors by default when hashline editing is enabled; edits should consume anchored views.
- Bash is for execution, not file IO/search/editing; blocked-command messages should point users to native tools.
- AST-grep replace must stay structural and previewable; do not fall back to regex-shaped rewrites through this surface.
- `ast_grep_replace` maps to `edit`, defaults to dry-run, rejects truncated apply, validates current bytes, and writes diff artifacts.
- LSP/MCP/network availability is optional in deterministic lanes; return actionable structured errors.
- `session_*` tools are replay-derived and must not run providers, hooks, network, MCP, or the CLI.
- `hashline_scan`/`hashline_apply` and `shell.run` are internal/coordinator-only; do not advertise them as public tool ids.
- Large outputs need artifact flow plus concise summaries; do not drop the full artifact when summarizing.

## TESTS
Owner test guidance, grouped suites, and shared fixtures live in `tests/AGENTS.md`. Run the crate suite with `cargo nextest run -p harness-tools`.

## ANTI-PATTERNS
- Do not duplicate permission policy here; enforce the decision `harness-core` made.
- Do not add a tool without schema coverage and a permission/capability story.
- Do not use ad hoc path joins for workspace files.
- Do not collapse full output into event summaries only; preserve artifact flow for large outputs.
- Do not make read/search/session tools mutate state.
- Do not advertise low-level compatibility helpers as public tool ids without catalog and documentation coverage.

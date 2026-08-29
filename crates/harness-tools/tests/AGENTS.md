# AGENTS: crates/harness-tools/tests

## OVERVIEW
Owner suites for the native tool surface. Every top-level `tests/*_test.rs` is a Cargo integration target; grouped suites include numbered part files from `tests/<suite>/` via `include!`. Shared fixtures live in `tests/common/`. In-crate unit tests sit beside source (`src/native_tools/tests.rs`, `src/fs_read/tests.rs`, `src/lsp_support/tests.rs`).

## SCHEMA GATES
| Test | Purpose |
|------|---------|
| `native_tool_schema_guidance_test` | High-risk provider-visible tool fields carry model guidance. |
| `native_malformed_args_recovery_test` | Malformed args return actionable recovery messages. |
| `integrations_matrix_test` | MCP and LSP families: one real boundary E2E each plus bad input, permission denial, process failure, cancellation/restart, redaction. |

## EXECUTION/EDIT OWNERS
| Test | Purpose |
|------|---------|
| `native_execution_surface_test` (`tests/native_execution_surface/`) | Registry execution, public `edit`, `write`/`apply_patch`/exact edit, apply_patch preflight, baseline shape compat, provider tool-def schemas. |
| `native_workspace_edit_routing_test` | Edit routing through hashline, symlink and workspace-path safety. |
| `hashline_apply_test` | Hashline anchor validation, overlap rejection, atomic apply. |

## SESSION/DISCOVERY OWNERS
| Test | Purpose |
|------|---------|
| `native_workspace_intelligence_tools_test` | Session inspection tools: symlink escape rejection, list filters/sort, replay-safe redacted capping. |
| `session_info_tool_test` | Replay-derived session info and capped/redacted summaries. |

## DELEGATION/QUESTION/SKILL OWNERS
| Test | Purpose |
|------|---------|
| `native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test` (`tests/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/`) | `task`/`batch`/`background_output` lineage, permissions, ordering, reentry, toolset boundary. |
| `native_agent_spawn_child_session_observability_test` | Child session observability from `task` spawns. |
| `native_control_plane_tools_test` | `question`/`skill`/todos and the `invalid` tool. |
| `native_question_tool_test` (`tests/native_question_tool/`) | Permission answers, reject/timeout behavior. |
| `skill_load_discovery_test` (`tests/skill_load_discovery/`) | Project/global discovery, custom roots, v1 skill contract. |

## LSP/AST/NETWORK/MCP OWNERS
| Test | Purpose |
|------|---------|
| `native_code_lsp_test` (`tests/native_code_lsp/`) | Configured/custom servers, direct file, rename previews, install decision. |
| `native_ast_grep_search_test`, `native_ast_grep_replace_test` | Structural search/replace, dry-run and diff artifacts. |
| `native_web_fetch_test`, `native_web_search_test`, `native_code_search_test`, `native_github_test` | Network/GitHub via injected mock transports (`coordinator_registry_with_*_transport` builders). |
| `mcp_generic_test` | MCP generic call surface against fake/stateful MCP servers (`tests/common/mcp_server.rs`). |

## TRUNCATION/ARTIFACT/SAFETY OWNERS
| Test | Purpose |
|------|---------|
| `native_read_truncation_presentation_test`, `native_grep_truncation_presentation_test` | Output truncation markers and presentation shape. |
| `native_large_output_dogfood_test` (`tests/support/native_large_output_dogfood_support.rs`) | Large-output artifact flow plus concise summaries. |
| `shell_timeout_boundary_test` | Shell timeout and cancellation racing timeout terminate command trees. |
| `single_surface_live_test` | Live single-surface execution through a spawned coordinator. |

## SHARED FIXTURES (`tests/common/`)
| File | Role |
|------|------|
| `workspace.rs`, `tool_context.rs` | Workspace fixtures and coordinator-backed `ToolContext` builders. |
| `event_log.rs`, `event_reader.rs`, `question_events.rs` | Event log readers and question-permission waiters. |
| `mcp_server.rs`, `remote_search_env.rs` | Fake MCP servers and remote-search test environment. |
| `repo_root.rs`, `single_surface_live.rs` | Repo-root discovery and live single-surface helpers. |
| `*_fixtures.rs` | Per-suite fixtures for execution surface, delegation, LSP, question, and skill suites. |
| `tests/support/` | Large-output dogfood support modules. |

## RULES
- The top-level `tests/*_test.rs` target is the ownership unit; edit parts under `tests/<suite>/` and `include!` them in the harness file.
- Prefer `tests/common/` fixtures over duplicating workspace/event setup in each suite.
- Truncation/artifact/safety owners pin presentation markers and artifact flow, not just event summaries.
- Assert the permission decision `harness-core` made; do not re-implement permission policy here.

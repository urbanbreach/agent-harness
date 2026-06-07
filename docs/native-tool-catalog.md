# Native tool catalog

Harness exposes one built-in native tool surface through `harness-tools`. The runtime registry is the source of truth; this document is a human-readable mirror kept honest by `native_tool_parity_matrix_test`.

Tool execution still goes through the coordinator permission path before the tool runs. `none` below means the tool is read-only local inspection or control-plane reporting and does not have its own public permission bucket; it still must be present in the active agent toolset.

| Tool id | Permission | Mutation | Replay / artifact behavior | Notes |
|---|---|---|---|---|
| `ast_grep_search` | `codesearch` | read-only | Capped JSON; large results spill to artifacts | Built-in ast-grep CLI structural search adapter. |
| `ast_grep_replace` | `edit` | workspace mutation | Dry-run/apply JSON plus diff artifacts; large JSON spills to artifacts | Built-in ast-grep rewrite adapter. Defaults to dry-run; the ast-grep process runs in JSON rewrite mode and never mutates the workspace directly; apply mode writes through Harness workspace path checks and atomic edit writes. |
| `background_cancel` | `task` | control-plane mutation | Coordinator cancellation events; output is replay-derived | Canonical explicit cancellation wrapper for background child requests. Alias: `background_output(cancel=true)`. |
| `background_output` | `task` | read/cancel compatibility | Replay-derived background status/result; `cancel: true` remains compatibility | Use for status/result retrieval; cancellation next-actions prefer `background_cancel`. Aliases: `task_id`, `session_id`. |
| `bash` | `bash` | host command | Captured output and artifacts when large | Shell allowlist and permission policy apply before execution. |
| `batch` | none | depends on child calls | Preserves source order for model-visible results | Executes multiple native tool calls through coordinator tool execution; each child call keeps its own permission check. |
| `codesearch` | `codesearch` | network/read-only | External I/O when called | Remote code-search integration. |
| `edit` | `edit` | workspace mutation | Hashline/diff artifacts | Normal file-changing route. |
| `github.issue` | legacy `network` compatibility | network mutation/read | External I/O when called | GitHub integration wrapper; not required for offline V1 claims. |
| `github.pull_request` | legacy `network` compatibility | network mutation/read | External I/O when called | GitHub integration wrapper; not required for offline V1 claims. |
| `glob` | none | read-only | Inline capped output | Workspace-safe file discovery. |
| `grep` | none | read-only | Large results spill to artifacts | Workspace-safe text search. |
| `invalid` | none | control-plane report | Summary only | Records malformed/unsupported tool calls as tool messages. |
| `list` | none | read-only | Inline capped output | Workspace-safe directory listing. |
| `lsp` | `lsp` | language read-only | Structured unsupported responses | Diagnostics/symbol/reference helpers. |
| `lsp.rename` | `edit` | workspace mutation | Rename/diff artifacts | LSP rename path remains edit-permission gated. Alias: `rename_symbol`. |
| `plan_enter` | `question` | control-plane question | Summary only | Requests Build → Plan handoff. |
| `plan_exit` | `question` | control-plane question | Summary only | Requests Plan → Build continuation. |
| `question` | `question` | user interaction | Summary only | Operator question/confirmation path. |
| `read` | none | read-only | Hashline anchors; large output spills | Workspace-safe file read. Aliases: `filePath`, `path`. |
| `session_info` | none | read-only | Replay-derived JSON; large output spills | Model-visible session metadata, lineage, event counts, artifacts, recovery notes. |
| `session_list` | none | read-only | Replay-derived JSON | Model-visible session catalog listing with filters/sort/caps. |
| `session_read` | none | read-only | Replay-derived JSON; large output spills | Bounded redacted event/message windows. |
| `session_search` | none | read-only | Replay-derived JSON; large output spills | Redacted search over safe replay-derived session text. |
| `shell.run` | `bash` | host command | Captured output and artifacts when large | Lower-level shell id kept canonical for compatibility tests. |
| `skill` | `task` | prompt/control-plane read | Summary plus loaded skill content | Loads configured markdown skills under skill permission rules. |
| `task` | `task` | child scheduling | Child session events and structured route/runtime metadata | Canonical subagent delegation tool. Aliases: `agent`, `subagent_type`. |
| `todoread` | `task` | read-only | Control-plane state output | Reads the run-local todo state. |
| `todowrite` | `task` | control-plane mutation | Run-local state output | Writes validated todo state. |
| `webfetch` | `webfetch` | network/read-only | External I/O when called | Fetches web content under permission policy. |
| `websearch` | `websearch` | network/read-only | External I/O when called | Searches web content under permission policy. |

## V1 control-plane additions

- `session_list`, `session_read`, `session_search`, and `session_info` are model-visible, redacted by default, capped, and side-effect free. They read existing session directories and event logs; they do not shell out to `harness sessions`, run providers, run tools, start MCP servers, or make network calls.
- `background_cancel` is the canonical cancellation id for a background child request. `background_output(cancel=true)` remains documented compatibility.
- `ast_grep_search` is read-only and maps to `codesearch`. It invokes the local `ast-grep` CLI in read-only mode with strict args, workspace path checks, explicit/safely inferred language, hard result/context/per-match caps, and artifact spill.
- `ast_grep_replace` maps to `edit` and defaults to dry-run. It invokes the local `ast-grep` CLI only for JSON rewrite planning, rejects traversal/unknown/unsupported args, refuses partial apply when results are truncated, validates adapter byte ranges against current file contents, and applies only through Harness workspace path checks, atomic writes, and diff artifacts.

## Bash safety

The `bash` wrapper default timeout is 120000 ms. The output cap is 2000 lines or 51200 bytes before full output is written to artifacts. The blocked command policy rejects shell search/read/edit shortcuts such as `find`, `grep`/`rg`, `cat`, `head`, `tail`, `sed`, and `awk`; use `glob`, `grep`, `list`, `read`, or `edit` instead. This guidance mirrors `shell_run.rs` and `shell_safety.rs`.

`ast_grep_replace` is advertised only as an edit-permission structural rewrite tool; use dry-run first and inspect the diff artifact before apply mode.

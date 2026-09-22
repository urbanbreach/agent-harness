# Native tool catalog

The `harness-tools` registry defines the native tools available to agents. This
table lists their IDs, permissions, side effects, and stored output.

The coordinator checks tool membership and permissions before execution. `none`
in the permission column means the tool has no separate public permission kind.
It still needs to be in the agent's toolset; nested calls keep their own checks.

| Tool id | Permission | Mutation | Replay / artifact behavior | Notes |
|---|---|---|---|---|
| `ast_grep_search` | `codesearch` | read-only | Capped JSON; large results spill to artifacts | Built-in ast-grep CLI structural search adapter. |
| `ast_grep_replace` | `edit` | workspace mutation | Dry-run/apply JSON plus diff artifacts; large JSON spills to artifacts | Built-in ast-grep rewrite adapter. Defaults to dry-run; the ast-grep process runs in JSON rewrite mode and never mutates the workspace directly; apply mode writes through Harness workspace path checks and atomic edit writes. |
| `apply_patch` | `edit` | workspace mutation | Sequential patch output plus diff artifacts | Applies add/update/delete patch text through Harness workspace path checks and atomic edit writes. Moves are rejected. |
| `background_cancel` | `task` | control-plane mutation | Coordinator cancellation events; output is replay-derived | Canonical explicit cancellation wrapper for background child requests. Supports `all: true` for bulk cancellation of non-terminal background tasks. Alias: `background_output(cancel=true)`. |
| `background_output` | `task` | read/cancel compatibility | Replay-derived background status/result; `cancel: true` remains compatibility | Use for status/result retrieval; cancellation next-actions prefer `background_cancel`. Supports `full_session`, `include_thinking`, `message_limit`, `since_message_id`, `include_tool_results`, `thinking_max_chars`, and `from_end` for rich child-session retrieval. Multi-wait: `request_ids` + `wait_mode` (`any`/`all`) with `block` for coordinator-owned wait-any/wait-all. Aliases: `task_id`, `session_id`. |
| `bash` | `bash` | host command | Captured output and artifacts when large | Shell allowlist and permission policy apply before execution. Globs, heredocs, interpreter command modes, file-descriptor redirects, and `/dev/null` redirects are allowed under permission-patterns mode; true out-of-workspace paths and working directories require `external_directory` approval. Catch-all bash deny removes the tool from the model-visible list. |
| `batch` | none | depends on child calls | Preserves source order for model-visible results | Executes multiple native tool calls through coordinator tool execution; each child call keeps its own permission check. |
| `codesearch` | `codesearch` | network/read-only | External I/O when called | Remote/public code-search integration; use `grep`, `ast_grep_search`, or `lsp` for local workspace symbols. |
| `edit` | `edit` | workspace mutation | Hashline/diff artifacts | Normal file-changing route. Also accepts exact `oldString`/`newString` edits. |
| `github.issue` | legacy `network` compatibility | network mutation/read | External I/O when called | GitHub integration wrapper; not required for offline V1 claims. |
| `github.pull_request` | legacy `network` compatibility | network mutation/read | External I/O when called | GitHub integration wrapper; not required for offline V1 claims. |
| `glob` | none | read-only | Inline capped output | Workspace-safe file discovery. Results sorted by modification time (newest first). |
| `grep` | none | read-only | Large results spill to artifacts | Workspace-safe text search. Supports `output_mode` (`content`, `files_with_matches`, `count`) and `head_limit` to cap files returned. |
| `invalid` | none | control-plane report | Summary only | Records malformed/unsupported tool calls as tool messages. |
| `list` | none | read-only | Inline capped output | Workspace-safe directory listing. |
| `lsp` | `lsp` | language read-only | Structured unsupported responses | Diagnostics/symbol/reference helpers. Supports `installDecision` operation for LSP server install consent. |
| `lsp.rename` | `edit` | workspace mutation | Rename/diff artifacts | LSP rename path remains edit-permission gated. Alias: `rename_symbol`. |
| `question` | `question` | user interaction | Summary only | Operator question/confirmation path. |
| `read` | `read` | read-only | Hashline anchors; large output spills | Workspace-safe file read. `.env` basename patterns ask by default; out-of-workspace paths use `external_directory`. Aliases: `filePath`, `path`. |
| `session_info` | none | read-only | Replay-derived JSON; large output spills | Model-visible session metadata, lineage, event counts, artifacts, recovery notes. |
| `session_list` | none | read-only | Replay-derived JSON | Model-visible session catalog listing with filters/sort/caps. |
| `session_read` | none | read-only | Replay-derived JSON; large output spills | Bounded redacted event/message windows. Supports `include_todos` and `from_end` params. |
| `session_search` | none | read-only | Replay-derived JSON; large output spills | Redacted search over safe replay-derived session text. |
| `shell.run` | `bash` | host command | Captured output and artifacts when large | Lower-level shell id kept canonical for compatibility tests. |
| `skill` | `read` | prompt/control-plane read | Summary plus loaded skill content | Loads configured markdown skills under read permission rules. |
| `task` | `task` | child scheduling | Child session events and structured runtime metadata | Named subagent delegation. New tasks require `subagent_type`, `prompt`, `run_in_background`, and `load_skills`; `description` and `command` are optional, while `task_id`/`session_id` continue a direct child. There is no category selector. |
| `todoread` | `task` | read-only | Control-plane state output | Reads the run-local todo state. |
| `todowrite` | `task` | control-plane mutation | Run-local state output | Writes validated todo state. |
| `webfetch` | `webfetch` | network/read-only | External I/O when called | Fetches web content under permission policy. |
| `websearch` | `websearch` | network/read-only | External I/O when called | Searches web content under permission policy. |
| `write` | `edit` | workspace mutation | Full-file write plus diff artifacts | Writes or creates exactly one file through Harness workspace path checks and atomic edit writes. |

## Tasks and session inspection

- `session_list`, `session_read`, `session_search`, and `session_info` are model-visible, redacted by default, capped, and side-effect free. They read existing session directories and event logs; they do not shell out to `harness sessions`, run providers, run tools, start MCP servers, or make network calls.
- `background_cancel` is the canonical cancellation id for a background child request. `background_output(cancel=true)` remains documented compatibility.
- `ast_grep_search` is read-only and maps to `codesearch`. It invokes the local `ast-grep` CLI in read-only mode with strict args, workspace path checks, explicit/safely inferred language, hard result/context/per-match caps, and artifact spill.
- `ast_grep_replace` maps to `edit` and defaults to dry-run. It invokes the local `ast-grep` CLI only for JSON rewrite planning, rejects traversal/unknown/unsupported args, refuses partial apply when results are truncated, validates adapter byte ranges against current file contents, and applies only through Harness workspace path checks, atomic writes, and diff artifacts.
- `codesearch` itself is a remote/public backend integration, not local-first symbol lookup. For first-party code, prefer `grep` for text, `ast_grep_search` for structural code, or `lsp` for language-server symbols/references.

## LSP verification

After changing source, use `lsp` with `operation: "fileDiagnostics"` and the changed
`filePath`. `write`, `edit`, `apply_patch`, and the hashline compatibility tools also
attach diagnostics for each surviving changed file. An edit still succeeds if the
language server is unavailable; the output and structured diagnostics report the
failed check. Unsupported extensions and explicitly disabled servers do not add
an automatic warning. Diff views retain the diagnostic result.

Language servers persist for the active run, shared by its agents, explicit LSP
calls, rename, and automatic edit checks. The pool keys connections by project
root and effective server configuration, admits at most six cached servers, evicts
the least recently used idle server when full, and closes servers after five idle
minutes or when the run ends. A cancelled or failed check discards its connection;
the next call can start a fresh server. No daemon, dependencies, or global pool.

Up to 200 open files are synchronized with increasing document versions; unchanged
text is not resent, supported save notifications are sent, and deleted or redirected
files are closed. Shutdown attempts the protocol handshake before a bounded kill. Pull diagnostics use result IDs and
accept unchanged reports only with a matching cache. Push diagnostics reject old
versions; versionless pushes settle for 250 ms. Missing, stale, malformed,
disconnected, or timed-out results are not clean checks. Calls have a 30-second
budget (including queueing) and a 10-second diagnostic wait. Cold rust-analyzer
startup waits for its readiness notification. Requests serialize per server;
independent servers run concurrently. Valid empty navigation results return
immediately without the former repeated retries.

`workspaceDiagnostics` checks files supported by the selected server through the
same per-file path, skipping `.git`, `target`, and `node_modules`. It rejects scans
over 200 files instead of returning a partial clean result. Prefer changed-file
checks for large projects, and use the compiler/tests when diagnostics are unavailable.

Run `scripts/qa/verify-lsp.sh` for protocol regressions, coordinator edit simulations,
release-mode rust-analyzer checks against `cargo check --offline`, and production TUI
frames rendered in xterm.js at 40, 80, and 120 columns. This opt-in lane requires
Python 3, rust-analyzer, Cargo/nextest, `/usr/bin/chromium`, and the existing
`scripts/qa` Node dependencies. It writes logs, durable events, diffs, ANSI frames,
screenshots, cold/warm call timings, process-reuse assertions, and a source-hash manifest under `.omo/evidence/`.

## Bash safety

The `bash` wrapper default timeout is 120000 ms. The output cap is 2000 lines or 51200 bytes before full output is written to artifacts. Shell commands are controlled by permission patterns and workspace path safety by default, not a static executable allowlist; a disallowed invocation is reported as a blocked command. Permission-pattern mode allows approved interpreter command modes such as `python3 -c` and heredocs, file-descriptor redirections such as `2>&1`, and literal executable discovery with `command -v` or `command -V`, while continuing to block general shell-wrapper and environment-dump commands. A standalone trailing `&` remains blocked because Harness does not detach untracked shell processes; use coordinator-owned background tasks for managed asynchronous work. Legacy-executables mode retains stricter interpreter-mode checks. Shell search/read/edit shortcuts such as `find`, `grep`/`rg`, `cat`, `head`, `tail`, `sed`, and `awk` are discouraged; use `glob`, `grep`, `list`, `read`, or `edit` instead. This guidance mirrors `shell_run.rs` and `shell_safety.rs`.

`ast_grep_replace` is advertised only as an edit-permission structural rewrite tool; use dry-run first and inspect the diff artifact before apply mode.

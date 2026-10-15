# Native tools

`coordinator_registry_with_skills` registers the tools below on Linux. This is
the base registry, not the tool list advertised to every model. Agent toolsets,
permission rules, eval routing, and edit mode can reduce that list. Configured
MCP servers add their helper operations and discovered tools.

The coordinator checks an agent's toolset and permission rules before running a
tool. Calls share the run's concurrency limit. Tools that wait for child work
release that capacity. Cancellation waits for tool cleanup before shutdown.

| Tool | Permission | Behavior |
| --- | --- | --- |
| `read` | `read` | Read UTF-8 text (1-based offset, at most 2,000 lines and 64 KiB before coordinator limits), a PNG, JPEG, GIF, or WebP image, or a retained PDF artifact. Hashline mode adds line/hash anchors. With `notes_windows` on, `history://current/full` reads the agent's whole branch history. |
| `list` | `read` | List up to 1,000 immediate directory entries in name order. |
| `glob` | `read` | Find paths with ignore-file support, newest first. It lists names without reading file contents. Default limit 100, accepted range 1 through 1,000. |
| `grep` | `read` | Search UTF-8 contents with ignore-file support; skip files that need separate read approval. Default limit and head limit 100, accepted range 1 through 1,000; context defaults to 0 and accepts up to 64 lines. With `notes_windows` on, the path `history://current/full` searches the agent's whole branch history. |
| `ast_grep_search`, `ast_grep_replace` | `read`, plus `edit` when applying | Structural search, previews, and checked rewrites using the installed ast-grep CLI. See [structural edits](ast-grep-replace-safety-gate.md). |
| `lsp` | `lsp` and `read` | Query an installed language server for symbols, definitions, references, hover, call hierarchy, and diagnostics. See [language tools](lsp.md). |
| `lsp.rename` | `lsp`, `read`, plus `edit` when applying | Preview or apply a semantic rename with path approvals, diffs and undo. |
| `write`, `edit` | `edit` | Create or edit UTF-8 files through coordinator-owned mutation checks. |
| `apply_patch` | `edit` | Apply add, update, or delete patches to at most 256 files, sequentially. Moves remain unsupported. |
| `bash` | `bash` | Execute an approved command with bounded output, process-group cleanup and optional filesystem confinement. See [shell commands](shell.md). |
| `monitor` | `bash` for commands; `read` for paths; `task` for rearm | Watch command lines or a file condition in a coordinator background task. See [monitor](monitor.md) for events, limits, and rearm. |
| `webfetch` | `webfetch` | Fetch bounded text, images, or binary artifacts. See [web fetching](web.md) for limits. |
| `websearch`, `codesearch` | Tool ID, matched against the query | Search public pages or code documentation through Exa. See [web tools](web.md). |
| `github.issue`, `github.pull_request` | Tool ID, matched against `owner/repo:operation` | Read and update issues or pull requests. See [GitHub tools](github.md). |
| `question` | `question` | Request an operator answer through the coordinator. |
| `skill` | `skill` | Load configured local skill instructions. Arguments are limited to 16 KiB; expanded instructions to 1 MiB. Tool hints do not grant permissions. |
| `spawn_subagent`, `task` | `spawn_subagent`, plus `task` matched against the resolved agent type | Start a child from a resolved definition, optionally resuming a completed conversation. `task` uses `run_in_background` instead of `background`. |
| `get_command_or_subagent_output`, `get_task_output` | `get_command_or_subagent_output` | Read owned child, background command, or monitor output. |
| `wait_commands_or_subagents`, `wait_tasks` | `wait_commands_or_subagents` | Wait for owned children, background commands, and monitors. |
| `kill_command_or_subagent`, `kill_task` | `kill_command_or_subagent` | Cancel an owned child, background command, or monitor. |
| `eval` | `eval` (allowed by default), plus each nested call | Persistent code cells, parallel tool composition, structured output, and background execution. See [eval](eval.md). |
| `todoread` | `task` | Read the current run's journaled todo list. |
| `todowrite` | `task` and `todowrite` | Replace the validated todo list. |
| `session_list`, `session_read`, `session_search`, `session_info` | Tool ID | Inspect existing session journals. See [session tools](sessions.md). |
| `context_notes`, `new_context` | `read` to read the notes; `edit` to replace them and for `new_context` | Keep persistent notes across context windows and start a fresh window. Present only with `runtime.compaction.notes_windows` on. See [context notes and new windows](#context-notes-and-new-windows). |

Paths outside the workspace also require `external_directory` approval. Native
edits cannot modify managed session storage or the permission-grant file. These
checks are application policy, not an operating-system sandbox.

Resolved CLI subagent settings replace the base registrations. Disabling spawning
removes `spawn_subagent` and `task`, but leaves output, wait, and kill tools
available for commands. Enabling active-agent messaging adds
`send_subagent_message`, with permission key `send_subagent_message` and
coordinator routing checks. It is absent from the default base registry.

`bash` and command monitors require Bash. Structural tools require `ast-grep`
on `PATH`; LSP queries and rename require a language server executable. Eval
requires the selected language runtime. Search requires access to the configured
Exa MCP endpoint. GitHub mutations require a token; public reads do not. Skill
loading requires a readable local skill in the configured catalog. See the linked
tool pages for setup and per-tool limits.

## Edits and output

Edits to existing files require a current read fingerprint. The coordinator keeps
bounded undo baselines and refuses to undo over a later external change. File
operations preserve existing permissions. Patch moves are unsupported. Semantic
rename supports file moves through `lsp.rename`.

`edit.mode` accepts `auto` (the default), `apply_patch`, `replace`, or `hashline`.
`edit.model_variants` overrides the mode by case-insensitive model substring;
the longest matching key wins. Auto selects patches for GPT, Codex, and numbered
o-series model names; exact replacement for Kimi, MiMo, MiniMax, DeepSeek,
StepFun, and GLM Flash; hashline otherwise. If the preferred editing tool is
unavailable, resolution uses the available patch or replacement tool.

Patch mode advertises `apply_patch` instead of `edit` and `write`. Replace and
hashline modes advertise `edit` and `write` instead of `apply_patch`. Replace
mode uses `oldString`, `newString`, and optional `replaceAll`. Hashline mode
also accepts up to 1,000 line edits with `replace`, `append`, or `prepend`,
`pos`/`end` anchors, and `lines`. Reads show `LINE#HASH` anchors only in
hashline mode. Each resulting UTF-8 file must fit within 8 MiB.

Tool results pass through the coordinator's redactor before storage and delivery.
Display text is shortened at 50 KiB or the 2,001st newline. Larger output goes
into a redacted artifact, with an 8 MiB retained-output limit. Structured fields
survive resume; most tools send them alongside display text. Subagent tools and
their aliases send display text to the provider without the structured wrapper.
Raw provider fragments remain transient. Settled assistant content is
retained for conversation continuity; support exports omit provider reasoning.
A `grep`, `glob`, `ast_grep_search` or `session_search` call that finds nothing,
and a `history://current/full` search with no matches, is flagged useless. With
`runtime.compaction.drop_useless` on, compaction can elide such results. A
`session_search` where every attempted session failed to load is not flagged,
because that is a failure the model should see.

Image results reach the provider through the same attachment path as prompt
images. The journal stores metadata and digests; private blobs hold the bytes.
Resume checks those digests without repeating the read or remote call. Rewind
removes attachments from the discarded conversation. File reads are limited to
8 MiB. A request may contain up to 16 text attachments and 16 MiB of attachment
bytes. Archive frames and inline imaging follow per-deployment image caps instead
of the 16-attachment count (see `crates/harness-providers/src/attachment_protocol/limits.rs`).
Provider limits can be stricter. PDFs and other binary downloads are retained as private artifacts; their contents are not extracted or sent as provider input.

## Context notes and new windows

`context_notes` and `new_context` exist only with `runtime.compaction.notes_windows`
on, and only for an unrestricted primary profile: one whose toolset has `read`,
`grep` and at least one of `edit`, `write`, `apply_patch` or `bash`. Subagents never
get them and an explicit toolset is never widened. With the setting off, both
tools are removed even from an explicit list.

`context_notes` with no arguments reads the notebook. With nothing saved it answers
"No context notes are stored for this session branch." With `text`, it replaces
the whole notebook, and an empty string clears it. Nothing is merged. The notebook
is plain UTF-8 text of at most 16 KiB (16,384 bytes), checked after redaction. A
larger one is refused with "Context notes are <n> bytes; the limit is 16384 UTF-8
bytes. Shorten the notebook and use history://current/full to recover raw detail."
A non-string `text` fails with "context_notes text must be a string." A save
returns "Context notes saved." with `entry_id`, `text` and `bytes`, and is refused
if the session branch changed while it ran. Each revision is a `ContextNotes`
journal record, so the notebook survives resume, and a rewind restores the earlier
revision.

`new_context` takes no arguments. Once the current tool calls finish, the worker
rolls the conversation over: older messages leave the live window, and the
notebook, the latest user or parent request and the most recent messages are
carried forward. It works even below the threshold and with automatic compaction
off, and it does not summarize anything.

`read` and `grep` resolve the path `history://current/full` to the caller's active
branch rendered as Markdown, including what the live window no longer holds. The
usual `offset`, `limit` and search arguments apply. Any other `history://` path
fails with "Invalid history://current route; use exactly history://current/full
(selectors may follow it)". With the setting off, the route answers
"history://current/full is available only when runtime.compaction.notes_windows is
enabled". See [configuration](../configuration/config.md#notes-backed-context-windows).

## Code execution

Use `eval` to compose tools in persistent JavaScript or Python cells. Ruby and
Julia are opt-in. Every nested tool call keeps the caller's identity, permission
checks, concurrency limits, and journal lineage. `parallel()` preserves input
order; use `Promise.allSettled()` when sibling failures should stay independent.
Cancellation reaches child calls and waits for cleanup. `display()` returns
images through the normal attachment path. See [eval](eval.md) for helpers,
background cells, runtime requirements, and the migration example.

## Todos

`todowrite` takes `todos`, a complete replacement list. Each item contains
`content`, `status`, and `priority`. Status is `pending`, `in_progress`,
`completed`, or `cancelled`; priority is `low`, `medium`, or `high`.
Omitted status and priority default to `pending` and `medium`.

A list may contain at most 100 items and one `in_progress` item. Its redacted JSON
must fit within 32 KiB. Empty lists clear the state. Invalid, denied, or cancelled
writes do not replace it.

Successful tool results are the stored state. Resume and rewind follow those
journal records; no separate todo file needs synchronization. Reads scan the
journal and retain only todo versions needed to resolve rewinds.

In the TUI, a live update that leaves pending or in-progress items opens the
todo pane above the transcript without taking focus. A pane opened this way
closes again once no item is open. `toggle_todos` (`Ctrl+t` by default) shows,
focuses, or hides it. After you hide it, later updates leave it closed until you
open it again.

## Delegation and MCP

`spawn_subagent` requires `prompt` and `description`, defaults to the
`task` definition, and runs in the background unless `background` is
false. Use the returned `subagent_id` with the output, wait, kill, and message
tools. `resume_from` creates a new child from completed context; messaging can
wake the existing identity. Child sessions have independent journals, artifacts,
and catalog entries. See [agents and tasks](../operations/generic-agent-and-tasks.md)
for ownership, retained context, startup skills, and the shared permission
exception to Reference's explicit skill preload behavior.

[MCP tools](mcp.md) describes lazy connection, discovery, shared permissions,
response limits, and shutdown behavior.

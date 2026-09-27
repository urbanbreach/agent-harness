# Native tools

The replacement registry contains the native tools below. Configured MCP
servers add their helper operations and discovered tools.

The coordinator checks an agent's toolset and permission rules before running a
tool. Calls share the run's concurrency limit. Tools that wait for child work
release that capacity. Cancellation waits for tool cleanup before shutdown.

| Tool | Permission | Behavior |
| --- | --- | --- |
| `read` | `read` | Read bounded UTF-8 text with line/hash anchors, a PNG, JPEG, GIF, or WebP image, or a retained PDF artifact. |
| `list` | `read` | List a directory in name order. |
| `glob`, `grep` | `read` | Find paths or text with ignore-file support; skip files that need separate read approval. |
| `ast_grep_search`, `ast_grep_replace` | `read`, plus `edit` when applying | Structural search, previews, and checked rewrites using the installed ast-grep CLI. See [structural edits](ast-grep-replace-safety-gate.md). |
| `lsp` | `lsp` and `read` | Query an installed language server for symbols, definitions, references, hover, call hierarchy, and diagnostics. See [language tools](lsp.md). |
| `lsp.rename` | `lsp`, `read`, plus `edit` when applying | Preview or apply a semantic rename with path approvals, diffs and undo. |
| `write`, `edit` | `edit` | Create or edit UTF-8 files through coordinator-owned mutation checks. |
| `apply_patch` | `edit` | Apply add, update, or delete patches. Moves remain unsupported. |
| `bash` | `bash` | Execute an approved command with bounded output, process-group cleanup and optional filesystem confinement. See [shell commands](shell.md). |
| `webfetch` | `webfetch` | Fetch bounded text, images, or binary artifacts. See [web fetching](web.md) for limits. |
| `websearch`, `codesearch` | Tool ID, matched against the query | Search public pages or code documentation through Exa. See [web tools](web.md). |
| `github.issue`, `github.pull_request` | Tool ID, matched against `owner/repo:operation` | Read and update issues or pull requests. See [GitHub tools](github.md). |
| `question` | `question` | Request an operator answer through the coordinator. |
| `skill` | `skill` | Load configured local skill instructions. Tool hints do not grant permissions. |
| `task` | `task` | Start or continue a child agent with its own profile and permissions. |
| `background_output`, `background_cancel` | `task` | Inspect or cancel owned child requests. |
| `batch` | `batch`, plus each nested call | Run up to 25 independent calls and return results in input order. |
| `todoread` | `task` | Read the current run's journaled todo list. |
| `todowrite` | `task` and `todowrite` | Replace the validated todo list. |
| `session_list`, `session_read`, `session_search`, `session_info` | Tool ID | Inspect existing session journals. See [session tools](sessions.md). |

Paths outside the workspace also require `external_directory` approval. Native
edits cannot modify managed session storage or the permission-grant file. These
checks are application policy, not an operating-system sandbox.

## Edits and output

Edits to existing files require a current read fingerprint. The coordinator keeps
bounded undo baselines and refuses to undo over a later external change. File
operations preserve existing permissions. Patch moves are unsupported, as in the original backend. Semantic rename supports
file moves through `lsp.rename`.

Tool results pass through the coordinator's redactor before storage and delivery.
Display text is capped at 50 KiB or 2,000 lines. Larger output goes into a redacted
artifact, with an 8 MiB retained-output limit. Structured tool fields reach the provider together with display text and survive
resume. Raw provider fragments and reasoning are not durable history.

Image results reach the provider through the same attachment path as prompt
images. The journal stores metadata and digests; private blobs hold the bytes.
Resume checks those digests without repeating the read or remote call. Rewind
removes attachments from the discarded conversation. File reads are limited to
8 MiB. A request may contain up to 16 attachments and 16 MiB of attachment bytes.
Provider limits can be stricter. PDFs and other binary downloads are retained as private artifacts; their contents are not extracted or sent as provider input.

## Batches

Pass `tool_calls`, an array of objects containing `tool` and `parameters`.
`args` and `arguments` are accepted aliases for `parameters`. Empty batches and
batches larger than 25 calls fail validation. Nested batches are rejected.

Each call keeps the parent's agent identity, its own permission check, and a
journal link to the batch. A denied or failed call does not cancel its siblings.
The result includes ordered `details`, plus `successful` and `failed` counts.
Cancelling a batch cancels its child calls and waits for their cleanup.
Images follow the same input order. A batch that exceeds the aggregate attachment
limit returns an error after all children finish cleanup.

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

## Delegation and MCP

New `task` calls require `subagent_type`, `prompt`, `run_in_background`, and
`load_skills`. Use the returned `session_id` to continue an owned child. A
continuation cannot change its profile. Background completions can wake the
parent with a follow-up turn. Child sessions have independent journals, artifacts
and catalog entries. `background_output` can return bounded child history; see
[task history and ownership](../operations/generic-agent-and-tasks.md).

[MCP tools](mcp.md) describes lazy connection, discovery, shared permissions,
response limits, and shutdown behavior.

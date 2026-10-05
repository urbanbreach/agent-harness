# Native tools

`lib.rs` assembles `harness_core::tool::Tool` implementations. Schemas use strict
object-root arguments. Display text and structured JSON serve different callers;
large results spill through the coordinator's private artifact path.

| Work | Source |
| --- | --- |
| File reads and edits | `files.rs`, `files/`, `hashline.rs`, `patch.rs` |
| Search and structural edits | `search.rs`, `ast_grep.rs`, `ast_grep/` |
| Shell processes | `shell.rs`, `shell/`, `process.rs` |
| Formatting and language servers | `formatters.rs`, `formatters/`, `lsp.rs`, `lsp/` |
| Delegation and code execution | `subagents.rs`, `eval.rs`, `eval/` |
| Skills and sessions | `skills.rs`, `skills/`, `sessions.rs`, `sessions/` |
| Network tools | `web.rs`, `remote_search.rs`, `mcp/`, `github.rs` |

The coordinator checks permissions and owns cancellation, edits, and durable
writes. Discovering additional paths does not grant access: request permission
through the coordinator before reading or editing them.

Canonicalize paths, reject escapes, and check fresh content before publication.
Apply edits through shared staging, formatting, undo, and attribution paths.
Never apply truncated structural results or overlapping/stale rename edits.

Reuse lazy process and transport sessions within a run; close them at shutdown.
Inspection must not replay tools, hooks, MCP, or provider calls. Do not persist
raw arguments, credentials, reasoning, or unchecked artifact bytes.

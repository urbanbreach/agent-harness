# Tool tests

Drive tools through a coordinator and the actual registry. Give fixtures a
temporary workspace and explicit permissions. Script providers and local HTTP,
MCP, or LSP peers instead of mocking away the boundary under test.

| Behavior | Target |
| --- | --- |
| Reads, edits, patch | `files.rs`, `patch.rs` |
| Formatting and language edits | `formatting.rs`, `lsp.rs` |
| Delegation, history, cancellation | `tasks.rs`, `background_output.rs` |
| Parallel results and permissions | `batch.rs` |
| Network transports | `mcp.rs`, `web.rs`, `remote_search.rs`, `github.rs` |
| Skills and inspection | `skills.rs`, `sessions.rs`, `todos.rs` |
| Actual subprocesses and platform probes | `binary_smoke.rs`, `native/` |

Extend existing tests before adding a new fixture. Assert useful behavior:
permission enforcement, ordering, bounded output, redaction, durable history,
and cleanup. Subscribe before triggering work and keep waits bounded.

Default tests must not need external services or installed native tools.
Opt-in tests must fail when their required environment is missing. Avoid retries,
fixed sleeps, process-global mutation, and snapshots that only repeat the code.
Use nextest.

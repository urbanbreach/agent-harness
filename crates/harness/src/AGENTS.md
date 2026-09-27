# CLI implementation

Use `CliIo` for output and `CliDeps` for environment, workspace, provider, clock,
and cancellation seams. `lib.rs` handles process-level status codes; handlers
return errors rather than exiting the process.

| Work | Source |
| --- | --- |
| Provider/tool construction | `bootstrap.rs`, `bootstrap/secrets.rs` |
| Catalog selection | `runtime_catalog.rs` |
| Streaming prompt output | `prompt/output.rs`, `prompt/run.rs` |
| Session continuation | `prompt/session.rs`, `sessions/operations.rs` |
| Replay indexing | `replay/index.rs` |
| Export redaction | `exports.rs`, `exports/` |
| TUI event flow | `tui/live_events.rs`, `tui/live_intents.rs` |

Prompt completion belongs to its agent-turn terminal event. A child result or
provider completion cannot finish the parent prompt. Live fragments do not
advance the durable replay position; recover lag from the journal.

Keep argument validation before session creation. After creating a run, all
failure and cancellation paths must stop it and release resources. Resume uses
the recorded workspace. Forks leave their source unchanged. Metadata and indexes
use atomic replacement; event journals remain append-only.

Do not expose raw credentials, reasoning, loaded skill bodies, or unredacted
arguments in exports. Discovery describes extensions without activating code.

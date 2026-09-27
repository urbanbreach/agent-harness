# CLI tests

Drive `harness::run` with buffered `CliIo` and injected `CliDeps`. Give each test
a temporary workspace, session directory, and configuration directory. Do not
change the process environment or working directory.

| Behavior | Target |
| --- | --- |
| Routing, auth, configuration | `cli.rs`, `schema.rs`, `inspect.rs` |
| Provider/tool turns | `prompt.rs`, `prompt_models.rs`, `subscription.rs` |
| Streaming and cancellation | `prompt_streaming.rs` |
| Replay, continuation, fork | `sessions.rs`, `rewind.rs` |
| Operator commands | `operators.rs`, `workspace_cli.rs`, `archives.rs` |
| Process and terminal behavior | `binary_smoke.rs`, `pty_happy_path_recorded.rs` |

Search existing coverage before adding a test. Extend the closest behavioral
case and name the plausible regression. Assert parsed output and durable events;
use prose assertions only for intentional text contracts. Local HTTP fixtures
may exercise protocol boundaries. Live services and native processes belong in
explicit opt-in checks.

Subscribe before triggering asynchronous work and use bounded waits. Keep
fixtures deterministic without retries or fixed sleeps. Run nextest, not
`cargo test`. Preserve terminal fixtures and snapshots.

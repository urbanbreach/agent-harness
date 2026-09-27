# CLI guide

`src/lib.rs` owns Clap routing and integer exit codes. `src/main.rs` only calls
`harness::run_os()`. Command handlers receive `CliIo` and `CliDeps`; use those
streams, environment values, providers, clocks, and cancellation tokens.

| Work | Source |
| --- | --- |
| Prompt and scenario execution | `src/prompt.rs`, `src/prompt/`, `src/run.rs` |
| Runtime construction and models | `src/bootstrap.rs`, `src/runtime_catalog.rs` |
| Credentials | `src/auth.rs`, `src/auth/` |
| Sessions and replay | `src/sessions.rs`, `src/sessions/`, `src/replay.rs` |
| Exports and archives | `src/exports.rs`, `src/archives.rs` |
| Configuration and readiness | `src/config_commands.rs`, `src/inspect.rs` |
| Interactive execution | `src/tui.rs`, `src/tui/` |

Keep runtime authority in the coordinator. Inspection and readiness must not
execute tools, hooks, providers, or network probes. Export scans fail before
publication when credentials are found. Unknown capacity and unsupported
operations need explicit outcomes.

Use `cargo nextest run -p harness --test <target>` for a scoped integration check.
Prefer in-process commands; use a real binary only for process behavior.

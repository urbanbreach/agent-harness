# agent-harness

Rust workspace for an event-sourced agent harness with:

## Configuration

The current public integration surface is documented in [`docs/config.md`](docs/config.md).
At the moment, Agent Harness exposes only `integrations.remote_search` for the built-in
`web_search` and `code_search` tools; generic `integrations.mcp.servers` configuration
is not part of the public runtime contract yet.

- a CLI entrypoint
- coordinator/runtime core
- provider adapters
- built-in native + compat tools
- a Ratatui TUI
- deterministic PTY/live verification lanes

## Quick start

Validate the shipped example config:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc config validate
```

Launch the interactive harness with the canonical plan -> build split:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc
```

The shipped config starts in the `plan` profile. After the user approves implementation, use `plan.exit` to hand off to `build`.

## Shipped workflow surfaces

- `configs/harness.example.jsonc` — canonical example config
- `docs/plan-build-workflow.md` — first-run and handoff docs
- `crates/harness-tools/tests/native_control_plane_tools.rs` — `plan.exit` behavior coverage
- `crates/harness-testkit/tests/live_proxy_e2e.rs` — shipped config/signoff coverage

## Common commands

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p harness --test config_schema_cli
cargo test -p harness-tools --test native_control_plane_tools
cargo test -p harness-tools --test opencode_compat_live
cargo test -p harness-testkit live_proxy_e2e
```

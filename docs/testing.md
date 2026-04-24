# Testing and signoff map

Use the narrowest test that proves a change, then run the workspace gates before release:

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`

## Drift checks

- `cargo test -p harness --test config_docs_reference` verifies public config docs against generated schemas.
- `cargo test -p harness --test event_docs_reference` verifies `docs/architecture.md` lists every `EventV1` variant from `harness-core`.

## Live parity lanes

The live proxy signoff order and required environment variables are documented in
`crates/harness-testkit/tests/README.live-proxy.md`. Keep that file and this map aligned when adding
CLI/TUI parity coverage or documenting known live-coverage gaps.

# Issues and Blockers for rust-agent-harness-foundation

## Active Blockers
None yet

## Resolved Issues
None yet

- [2026-03-02T08:02:00Z] `harness-tui` unit tests failed due to missing committed insta snapshots; resolved by adding `*.snap` baselines for events/output/diff/help tabs and rerunning tests.

## Potential Gotchas

1. **Hashline CRLF handling**: Must strip trailing \r before hashing
2. **Tokio JoinSet ordering**: Completion order is NOT deterministic; use seq
3. **Crossterm event APIs**: Don't mix EventStream with poll/read
4. **Deterministic time**: FakeClock returns None for system_time in deterministic mode
5. **Permission timeout**: Headless default-deny, TUI waits for user

## CI Notes
- Rust jobs use `rust:latest` image
- PTY E2E requires Linux
- Use `INSTA_UPDATE=no` for snapshot stability checks
- Use `RUST_TEST_THREADS=1` for PTY tests (determinism)

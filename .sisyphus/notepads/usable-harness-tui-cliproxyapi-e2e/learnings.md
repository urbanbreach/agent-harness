# Learnings and Conventions

## Project Conventions

### Rust Patterns
- Use serde for serialization with #[serde(default)] for optional config sections
- Event-sourced architecture with JSONL append-only logs
- Coordinator pattern for central scheduling

### Config Handling
- Config files use JSONC (JSON with comments)
- Env substitution: ${VAR} = required, ${VAR:-default} = optional with fallback
- Config resolution order: --config flag → ./harness.jsonc → $XDG_CONFIG_HOME/harness/config.jsonc

### Testing
- Unit tests: cargo test --workspace
- Integration: INSTA_UPDATE=no cargo test --workspace --all-features
- PTY E2E: RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e

## Critical Decisions from Plan

1. **Interactive UX**: Prompt input + streaming transcript + activity + inspector panes
2. **Provider**: CLIproxyAPI with Responses API support (Codex-aligned)
3. **Config expansion**: Add optional `ui`, `logging`, `keybindings` (additive)
4. **MVP constraint**: Single default provider only

## File Locations
- Config: crates/harness-core/src/config.rs
- Coordinator: crates/harness-core/src/coord.rs
- Events: crates/harness-core/src/event.rs
- Provider: crates/harness-providers/src/openai.rs
- TUI: crates/harness-tui/src/
- CLI: crates/harness/src/

## 2026-03-03 Schema Expansion Notes

- `HarnessConfig` now supports optional `ui`, `logging`, and `keybindings` sections behind `#[serde(default)]` for backward compatibility.
- `openai_compatible` provider config now includes `api_mode` enum (`responses`, `chat_completions`, `auto`) with default `chat_completions`.
- `keybindings` remains a simple `BTreeMap<String, String>` with fixed default action keys (quit/focus/palette/help/follow/submit/clear/scroll/tab actions).

## 2026-03-03 Live update backpressure notes

- Live TUI transport now uses `crossbeam_channel::bounded(2048)` to cap UI backlog and avoid blocking async event forwarding.
- Display-path `ProviderStreamDelta` events are coalesced per `request_id` for up to 16ms or 1024 characters before enqueue, reducing high-frequency token churn without touching persisted JSONL events.
- When the queue is saturated, delta updates are dropped first and an overload banner (`UI overloaded: dropped N deltas`) is rate-limited to once per second.

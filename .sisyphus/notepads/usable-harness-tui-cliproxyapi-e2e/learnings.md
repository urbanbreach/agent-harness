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


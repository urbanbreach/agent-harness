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


## 2026-03-03 Task 18: Documentation refresh

### Completed Updates

#### README.md Changes
1. Updated "Launch the TUI" section to "Launch the Interactive TUI" with:
   - Basic `cargo run -p harness -- tui` command
   - Scenario-based TUI launch
   - Replay and sessions list commands

2. Added "Headless Prompt (Non-TUI)" section with:
   - Basic prompt command example
   - Custom config example
   - Output to file example

3. Added "CLIproxyAPI Quickstart" section with:
   - Config example using `api_mode: "responses"`
   - Environment variable setup (`OPENAI_API_KEY`)
   - Explanation of `api_mode` options (responses, chat_completions, auto)

4. Updated License Hygiene section:
   - Added note about MIT repos being fine for inspiration
   - Added note about Pi Agent Rust license being unclear; do not copy code

#### docs/config.md Changes
1. Updated Providers section:
   - Added `api_mode` field documentation with all three options

2. Added new config sections:
   - UI Settings (default_profile, theme)
   - Logging Settings (level, file, span_events)
   - Keybindings (customizable keyboard shortcuts)

3. Updated JSON Schema Reference:
   - Added ui and logging to HarnessConfig table
   - Added api_mode to ProviderConfig table
   - Added new ApiMode enum section

#### Code Fix
Fixed compile error in `crates/harness-core/src/proj.rs`:
- Added missing `EventV1::UserMessageSubmitted(_)` match arm in `event_type_name` function

### Files Modified
- README.md
- docs/config.md
- crates/harness-core/src/proj.rs (compile fix)

### Evidence
- `.sisyphus/evidence/task-18-help.txt` - CLI help output (exit code 0)

### Notes
- The `prompt` command is documented but not yet implemented in the CLI
- The `api_mode` field is documented but the config struct doesn't have it yet (needs Task 3 implementation)
- All documented commands that exist work correctly

## 2026-03-03 Task 15: PTY E2E updates for prompt-first layout + interactive streaming

- PTY startup readiness marker is now `Prompt` (not `Tabs`) for deterministic readiness.
- Permission modal marker text is now `Permission Requested` (with space), requiring snapshot marker and focus-anchor updates.
- Diff tab content is currently represented by `diff artifact missing:` in this tree; visual snapshots should anchor to stable, non-path text to avoid hash drift from temp-path differences.
- Interactive prompt submission in PTY requires focus handoff to prompt pane (`Tab`, `Tab`) before typing/submitting.
- Added offline wiremock SSE fixture (`response.created`, two `response.output_text.delta`, `response.completed`) plus PTY checkpointing for streamed `Hello world` output.

## 2026-03-03 Task 18: Documentation Refresh (Final)

### Summary
Final documentation pass to ensure README and config docs reflect the implemented TUI, prompt commands, and CLIproxyAPI integration.

### Files Modified
- `docs/config.md` - Added CLIproxyAPI quickstart section and License hygiene note
- `README.md` - Verified existing content was correct (no changes needed)

### Key Documentation Additions

#### CLIproxyAPI Quickstart Section (docs/config.md)
- Default base_url: `http://127.0.0.1:8317/v1`
- Model example: `gpt-5-codex` (configurable)
- Env var: `OPENAI_API_KEY`
- Config example with `api_mode: "responses"`

#### License Hygiene Note (docs/config.md)
- MIT repos (Oh My OpenCode) fine for inspiration
- Pi Agent Rust license unclear → do not copy code

### Verification
- `cargo run -p harness -- --help` completed successfully (exit 0)
- Output saved to `.sisyphus/evidence/task-18-help.txt`
- Build had only minor unused import warnings (no errors)

### Documentation Coverage
All config sections now documented:
- `api_mode` (responses/chat_completions/auto)
- `ui` section (default_profile, theme)
- `logging` section (level, file, span_events)
- `keybindings` section (quit, submit, cancel, etc.)

## 2026-03-03 Task 17: Secret regression scan for artifacts

- Added a reusable testkit helper at `crates/harness-testkit/src/secret_scanner.rs` that recursively scans directory trees and skips symlinks for deterministic/safe traversal.
- Standardized forbidden patterns for regression checks:
  - regex: `sk-[A-Za-z0-9]{10,}`
  - substring: `Authorization: Bearer`
  - substring: `Bearer sk-`
- Added integration test `secretscan::secret_scan_does_not_find_api_keys_in_artifacts` to scan:
  - PTY temp session dirs under `${TMPDIR}/harness-testkit/pty-e2e-*`
  - `target/pty-visual-artifacts/`
  - all `crates/**/snapshots/` directories
- Kept failure output path/line/pattern only (no raw matched content) to avoid re-leaking secrets in CI logs.

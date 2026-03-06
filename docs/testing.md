# Testing

Agent Harness uses a three-level testing pyramid: unit, integration, and PTY E2E.

## Test Levels

### Unit Tests

Fast, isolated tests for individual modules:

```bash
cargo test -p harness-core clock
cargo test -p harness-core redact
cargo test -p harness-core hashline
cargo test -p harness-tools
```

Characteristics:
- No I/O or network
- Fake clock for deterministic time
- In-memory event store

### Integration Tests

Cross-module tests with real filesystem:

```bash
cargo test -p harness-core store
cargo test -p harness-core coord
cargo test -p harness-core perm
cargo test -p harness-tui
```

Characteristics:
- Temporary directories via `tempfile::TempDir`
- JSONL file store
- Snapshot testing with `insta`

### PTY E2E Tests

Full terminal UI tests using portable-pty:

```bash
RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e
```

Characteristics:
- Spawns real `harness` binary in PTY
- vt100 terminal parsing
- Keystroke injection
- Pixel-level screenshot artifacts + visual hash assertions

### Agent-visible visual artifacts

PTY E2E renders terminal cells to deterministic PNG images at key checkpoints:

- `pty_permission_requested.png`
- `pty_run_finished.png`
- `pty_diff_tab.png`

Default output directory:

```bash
target/pty-visual-artifacts/
```

Override output directory (useful in CI/artifact upload scripts):

```bash
HARNESS_VISUAL_ARTIFACT_DIR=/tmp/harness-visuals \
  RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e
```

Optional rendering controls for native-like text quality:

```bash
# Use a specific monospace TTF to match your local terminal font
HARNESS_VISUAL_FONT_PATH="/path/to/your/terminal-font.ttf" \
  RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e

# Toggle TTF anti-aliasing path (default: enabled)
HARNESS_VISUAL_TTF_ANTIALIAS=0 \
  RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e
```

The `.snap` files store marker checks plus `focus_pixels_blake3` digests for deterministic visual regression checks.

In GitLab CI, `rust:pty_e2e` exports `HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts` and always publishes these PNG artifacts so agents can inspect real rendered frames.

### Agent UX review workflow

For agent-driven UX iteration:

1. Run PTY E2E to generate deterministic screenshots.
2. Inspect `target/pty-visual-artifacts/*.png` for visual regressions.
3. Use `.snap` files to detect focus-region visual hash changes.
4. Adjust UI code and re-run PTY E2E until both image inspection and hashes are stable.

## Deterministic Environment

For reproducible tests, set these environment variables:

```bash
export HARNESS_DETERMINISTIC=1        # Use fake clock, stable run_id
export HARNESS_DISABLE_ANIMATIONS=1   # Disable TUI animations
export HARNESS_SEED=42                # Deterministic seed (optional)
export TZ=UTC                         # Stable timezone
export LANG=C.UTF-8                   # Stable locale
export LC_ALL=C.UTF-8
export TERM=xterm-256color            # Stable terminal type
```

### Single-Threaded Testing

PTY tests must run single-threaded to avoid terminal interaction conflicts:

```bash
RUST_TEST_THREADS=1 cargo test pty_e2e
```

### Deterministic Run Verification

Golden path should produce identical JSONL across runs:

```bash
# Run twice and compare
HARNESS_DETERMINISTIC=1 cargo run -p harness -- run --scenario golden_path --deterministic --out run1.jsonl
HARNESS_DETERMINISTIC=1 cargo run -p harness -- run --scenario golden_path --deterministic --out run2.jsonl
diff run1.jsonl run2.jsonl  # Should be empty
```

## Snapshot Testing

The project uses `insta` for snapshot testing. Guidelines:

### Creating Snapshots

```bash
# Update snapshots during development
cargo test --workspace --all-features

# In CI, fail on snapshot mismatch
INSTA_UPDATE=no cargo test --workspace --all-features
```

### Redactions

Sensitive data must be redacted from snapshots:

```rust
insta::with_settings!({
    filters => vec![
        (r"sk-[A-Za-z0-9]{10,}", "[REDACTED_API_KEY]"),
    ]
}, {
    insta::assert_snapshot!(output);
});
```

### Snapshot Locations

- Inline snapshots: `insta::assert_snapshot!()`
- File snapshots: `insta::assert_snapshot!("name", output)`
- Stored in `src/snapshots/` or `tests/snapshots/`

## Test Organization

```
crates/
  harness-core/
    src/
      clock.rs          # Unit tests in #[cfg(test)]
      redact.rs         # Unit tests in #[cfg(test)]
    tests/
      coord.rs          # Integration tests
      store.rs          # Integration tests
  harness-tui/
    tests/
      snapshots/        # insta snapshots
  harness-testkit/
    tests/
      pty_e2e.rs        # PTY end-to-end tests
    fixtures/
      mock_provider/    # Mock provider responses
```

## Running Specific Test Categories

```bash
# All unit tests
cargo test --lib

# All integration tests
cargo test --test '*'

# Specific crate
cargo test -p harness-core

# Specific test pattern
cargo test hashline
cargo test permission
cargo test coordinator

# With all features enabled
cargo test --workspace --all-features

# Offline only (no live proxy tests)
cargo test --workspace

# Live proxy tests (explicitly gated)
# HARNESS_LIVE_PROXY_CONFIG defaults to configs/harness.example.jsonc
# HARNESS_LIVE_PROXY_PROVIDER defaults to "default"
# OPENAI_API_KEY is optional for local CLIProxy when placeholder key fallback is used
# HARNESS_LIVE_PROXY_MODEL can force a specific real provider model
# HARNESS_LIVE_PROXY_PROMPT customizes the smoke prompt text
# HARNESS_LIVE_PROXY_WAIT_TIMEOUT_MS controls live smoke wait timeout (default 120000)
# Sign-off: run both the prompt smoke and the redesigned TUI smoke
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.3-codex \
cargo test -p harness-testkit live_proxy_prompt_responses_smoke -- --ignored --exact

HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.3-codex \
cargo test -p harness-testkit live_proxy_tui_responses_smoke -- --ignored --exact

# Optional: run the whole ignored live proxy suite in one shot
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.3-codex \
cargo test -p harness-testkit live_proxy_e2e -- --ignored

# Provider-layer live smoke (optional, same env gate)
HARNESS_LIVE_PROXY=1 cargo test -p harness-providers openai -- --ignored
```

## CI Testing

The CI pipeline runs:

```bash
# Format check
cargo fmt --check

# Linting
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Unit and integration tests
INSTA_UPDATE=no cargo test --workspace --all-features

# PTY E2E (Linux only, single-threaded)
RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e
```

## Writing New Tests

### Unit Test Template

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature() {
        let clock = FakeClock::new();
        let result = do_something(&clock);
        assert_eq!(result, expected);
    }
}
```

### Integration Test Template

```rust
use tempfile::TempDir;
use harness_core::store::JsonlFileEventStore;

#[tokio::test]
async fn test_store_append() {
    let dir = TempDir::new().unwrap();
    let store = JsonlFileEventStore::new(dir.path()).await.unwrap();
    // Test implementation
}
```

### PTY E2E Test Template

```rust
#[test]
fn test_golden_path_interactive() {
    let pty = spawn_pty("harness tui --scenario golden_path_interactive --deterministic");
    
    // Wait for permission prompt
    pty.wait_for_text("PermissionRequested", Duration::from_secs(5));
    
    // Send approval
    pty.send_key('a');
    
    // Wait for completion
    pty.wait_for_text("RunFinished", Duration::from_secs(10));
    
    // Take snapshot
    insta::assert_snapshot!(pty.screen_contents());
}
```

## Debugging Failed Tests

### Snapshot Mismatches

```bash
# Review pending snapshots
cargo insta review

# Accept all pending
cargo insta accept

# Reject all pending
cargo insta reject
```

### PTY Test Failures

Enable debug output:

```bash
RUST_LOG=debug RUST_TEST_THREADS=1 cargo test pty_e2e -- --nocapture
```

Inspect rendered checkpoint PNGs:

```bash
ls target/pty-visual-artifacts
```

### Determinism Failures

Check for:
- Unordered HashMap iteration (use BTreeMap)
- Wall clock usage (use Clock trait)
- Random values without seeded RNG
- Async timing assumptions

## Test Fixtures

### Mock Provider Fixtures

Located in `crates/harness-testkit/fixtures/mock_provider/`:

```json
{
  "request_digest": "abc123...",
  "responses": [
    {"Start": {}},
    {"TextDelta": "Hello"},
    {"Done": {"usage": {"prompt_tokens": 10}}}
  ]
}
```

### Golden Path Fixtures

- `golden_path` - Headless scenario with auto-resolved permissions
- `golden_path_interactive` - Interactive scenario requiring TUI approval

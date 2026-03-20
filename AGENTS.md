# AGENTS: agent-harness

## OVERVIEW
Rust workspace for an event-sourced agent harness with a CLI entrypoint, coordinator/runtime core, provider adapters, built-in tools, a Ratatui TUI, and deterministic PTY/live verification.

## CRATE MAP
| Crate | Responsibility | Start here |
|------|----------------|------------|
| `crates/harness` | CLI entrypoint, startup/replay orchestration, interactive mode wiring | `src/main.rs`, `src/tui.rs` |
| `crates/harness-core` | Event schema, coordinator, scheduler, permission policy, projections | `src/lib.rs`, `src/coord.rs`, `src/event.rs` |
| `crates/harness-providers` | Mock and OpenAI-compatible provider adapters | `src/lib.rs`, `src/openai.rs` |
| `crates/harness-tools` | Built-in tool registry and filesystem/shell/hashline tools | `src/lib.rs` |
| `crates/harness-tui` | Ratatui live/replay shell, layout/theme contracts, transcript rendering | `src/lib.rs`, `src/app.rs`, `src/ui.rs` |
| `crates/harness-testkit` | Secret scanning plus deterministic PTY/live verification helpers | `src/lib.rs`, `tests/` |

## WHERE TO LOOK
- Architecture overview: `docs/architecture.md`
- Test matrix and deterministic env: `docs/testing.md`
- TUI-specific workflow/contracts: `crates/harness-tui/AGENTS.md`
- PTY/live verification workflow: `crates/harness-testkit/tests/AGENTS.md`

## EDIT BOUNDARIES
- Keep runtime/event invariants in `harness-core`; do not let UI code invent state transitions locally.
- Keep layout and theme contracts in `harness-tui/src/layout.rs` and `src/theme.rs`; avoid scattering geometry rules.
- Keep test-only helpers in `crates/harness-testkit/tests/support/` or `#[cfg(test)]` modules, not runtime crates.
- Prefer extraction over redesign in oversized files: preserve public behavior while shrinking edit surfaces.

## SEARCH & SCOPE DISCIPLINE
- Primary workspace code lives under `crates/`, `configs/`, and `docs/`.
- Treat these as search noise unless the task explicitly needs them: `target/`, `inspirations/`, `.git/`, `.sisyphus/evidence/`.
- Check subtree `AGENTS.md` files before editing TUI or PTY test areas.

## VERIFICATION MATRIX
| Change type | Minimum verification |
|------------|----------------------|
| Core/runtime (`harness-core`, `harness-tools`, `harness-providers`) | `cargo fmt --all -- --check`, `cargo check --workspace`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, targeted crate tests |
| TUI/runtime shell (`harness-tui`, `harness/src/tui.rs`) | Above + `cargo test -p harness-tui` |
| PTY/live visual helpers | Above + `cargo test -p harness-testkit --tests -- --test-threads=1` |

## KNOWN VERIFICATION STATE
- The strict baseline is `fmt`, `check`, `clippy`, `harness-core`, and `harness-tui` all green.
- The `harness-testkit` serial PTY lane currently has a known failure cluster in 10 snapshots where markers stay present but `focus_pixels_blake3` / `focus_region_cells` drift by one cell.
- Treat PTY drift as a maintainability problem to be fixed deliberately; do not casually update snapshots without confirming the rendered shell contract still holds.

## CONVENTIONS
- Prefer typed helper structs over widening parameter lists when coordination/event APIs get large.
- Favor crate-root `//!` docs and local module docs to explain ownership and invariants near code.
- Keep deterministic test env intact: `HARNESS_DETERMINISTIC=1`, `HARNESS_DISABLE_ANIMATIONS=1`, `HARNESS_SEED=42`, `RUST_TEST_THREADS=1` where documented.

## ANTI-PATTERNS
- Do not weaken PTY/live verification to make brittle tests pass.
- Do not add alternative rendering stacks beyond the documented TUI/testkit dependencies.
- Do not mix production wiring with large inline test scaffolding when a sibling `tests` module is sufficient.

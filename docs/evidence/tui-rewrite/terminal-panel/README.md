# Terminal-panel scroll ownership

Based on `1ea17502`. Home previously read a scroll limit stored by painting in a `Cell`. Before the first paint it remained at the bottom; after resize it could use the old width's row count. The pinned original contains the same `Cell::set`/`Cell::get` paths. R10 records the correction.

Home now derives the limit from the same panel block, row builder and wrapper as painting. Frame preparation retains the last drawable content rectangle for temporarily hidden panels. It stores no duplicated output or rendered rows. Painting no longer changes this state. Terminal formatting itself remains in place during migration.

The existing navigation check now drives real interactive-PTY event fixtures. It checks Home before painting, wrapping changes between 140 and 60 columns, PageDown, zero-height round trips and transcript independence. `red.log` reproduces the original stale-limit failure. The first fix exposed a hidden-panel regression (`zero-red.log`); retaining drawable geometry fixes it (`zero-green.log`). No new test was added.

The ten scoped terminal checks pass. The 539 frozen checkpoints equal the prior approved candidate exactly, including its existing R8 corrections. Scoped Clippy, workspace check, formatting and test-suite gates are recorded alongside source hashes and the patch. This small correction has no new browser or release-performance claim; the preceding transcript-outline slice retains those measurements and their limitations.

```sh
cargo nextest run --profile ci -p harness-tui --all-features -j1 -E 'test(terminal_panel) | test(terminal_wrap) | test(terminal_truncation)'
HARNESS_TUI_REFERENCE_FRAMES=/tmp/terminal-frames cargo nextest run --profile ci -p harness-tui --all-features -j1 --test rewrite_reference_test
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
```

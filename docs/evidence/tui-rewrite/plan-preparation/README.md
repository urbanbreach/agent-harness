# Plan filesystem preparation

Based on `f9bd48e6`. Plan painting, pointer geometry and dashboard probe presence
now borrow one directory snapshot. Opening either surface and preparing an
already-demanded frame refresh it. Plan actions also reread the directory;
deletion retains replay, confinement and symlink validation. No timer, watcher,
backend change or dependency is added. Public `plan_view_rows` and
`plan_view_summary` remain fresh disk queries for callers that inspect files.

The source diff removes 91 lines, including 49 net test lines. Plan operations
share workspace/selection handling and keep the same preview/copy/error behavior.
The 4,000-scalar preview limit remains unchanged. The source tree has 556 Rust
files / 169,182 lines under `src` (including inline tests), and 118 files / 27,750
lines under `tests`. Plan painting and its scalar-based text truncation are not
yet replaced; this is a paint-purity step in the unfinished rewrite.

## Verification

The existing multi-plan journey adds a file after frame preparation. The
preceding implementation changes its buffer on the next paint and fails the
new assertion (`red.log`). Both its plan state and plan renderer files were
still identical to the pinned original (`red-source.json`). With the change,
repeated painting is stable, public disk queries immediately see the file, and
the next preparation refreshes the frame. Three redundant open/close/palette
tests are removed; the extended navigation journey and recorded oracle retain
those paths. The extended journey now opens the plan list through actual
palette keystrokes as well.

Independent review found that `/new` hides the dashboard without dropping its
stored value. The first candidate therefore still queried plans on hidden frames.
The same journey counts workspace lookups and fails on that candidate
(`hidden-red.log`). Gating preparation on `status_dashboard_is_active()` fixes
it (`hidden-green.log`); `hidden-regression.patch` reproduces the bad guard.
`initial-full.log` is the passing run before this review finding. `full.log`
validates the corrected production source. The final test assertion cleanup and
move into its existing navigation helper pass in `final-focused.log`.

All 1,681 TUI tests pass, with seven skips. Scoped all-target/all-feature Clippy,
workspace check, formatting and suite gates pass. All 543 complete frame records
equal the preceding candidate; the oracle still compares to the original matrix
with its explicit R8 correction. The golden file is unchanged. All 20 plan ANSI
frames match the original. Four plan list/preview captures at 40×24 and 120×40
have byte-identical PNGs and identical terminal display/cell fields. Two
representative candidate images were visually inspected. The browser
`renderCount` differs by one in each pair and is retained in the comparison.

No new performance, PTY or end-to-end latency measurement is claimed. Existing
Unicode truncation limitations and whole-rewrite acceptance remain outstanding.

## Reproduction

```sh
HARNESS_TUI_REFERENCE_FRAMES=/tmp/tui-plan-preparation-final HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/tui-plan-preparation-journeys cargo nextest run --profile ci -p harness-tui --all-features -j1
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
node scripts/qa/render-recorded-frames.mjs /tmp/tui-plan-preparation-xterm/reference .omo/evidence/tui-rewrite/plan-preparation/reference --source-root /home/urbanbreach/.codex/worktrees/tui-reference/agent-harness
node scripts/qa/render-recorded-frames.mjs /tmp/tui-plan-preparation-xterm/candidate .omo/evidence/tui-rewrite/plan-preparation/candidate
```

Each browser input contains the four `frames/*.ansi.gz` files decompressed plus
`producer.json` identifying the AppState `/view-plan`, Enter and fixed
reduced-motion clock. The reference ANSI comes from the unchanged original
recorder captured for `dashboard-cleanup`; its fixture, executable and source
receipts are retained there. `binaries.json`, `source.json`, `source.patch.gz`
and `files.json` identify the tested code and published artifacts.

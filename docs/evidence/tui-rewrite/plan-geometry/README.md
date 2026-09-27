# Plan painter and Unicode geometry

Based on `f1c7d3bc`. The plan painter is replaced, shrinking from 203 to 145 lines.
It borrows prepared entries and retains popup geometry, titles, styles, metadata,
selection, hover, scrolling and empty states. Paths and previews now clip whole
graphemes by display cells. This corrects R13: wide paths previously crowded out
metadata, and scalar clipping shortened combining/emoji rows or hid the ellipsis.

Independent review found that the shared UI prefix helper used composer widths,
which undercount VS16 emoji presentation sequences. The helper now uses the same
Unicode width library as Ratatui. Its terminal-panel and Bash-title callers retain
whole oversized graphemes. Terminal wrapping starts a fresh row when a grapheme
cannot fit the remaining cells, but keeps a leading zero-width prefix on that row.
Composer editing geometry, backend contracts and dependencies are unchanged.

Production code shrinks by 57 lines. Extensions to two existing wrapping tests
add 52 inline test lines; the public plan journey adds 108 integration-test lines.
The source tree contains 556 Rust files / 169,177 lines including inline tests,
and 119 files / 27,858 lines under `tests`. The whole rewrite remains incomplete.

## Checks and comparisons

The new journey opens `/view-plan` and then its preview through actual AppState
keystrokes at 40×24, 80×24, 120×40 and 160×50. It records a wide filename and
four preview rows: CJK, combining text, joined emoji and VS16. All eight original
records equal the pre-change candidate. The old painter fails metadata placement
in `red.log`. The first replacement fails the VS16 ellipsis assertion in
`vs16-red.log`. Neither failure is accepted as intended working behavior.

The extended wrapping checks fail on the old shared helper (`wrapping-red.log`).
They also caught partial-row overflow in the first fix (`partial-row-red.log`).
Review's zero-width-prefix case fails before the corrected cell-count guard
(`zero-width-red.log`). All four final focused checks pass in `focused.log`.
`initial-full.log` predates that last guard correction; `initial-clippy.log`
records decomposed test literals rejected by the existing NFC lint. Unicode
escapes retain the decomposed input without suppressing the lint.

The final full run passes 1,682 tests with seven skips. Scoped all-target,
all-feature Clippy, workspace check, formatting and suite gates pass. The
543-record matrix is unchanged from the preceding candidate, including cursor,
inputs and intents; the original golden file remains unchanged and its existing
R8 allowance remains explicit.

Eight paired xterm captures use the same emulator, browser, font, dimensions,
fixture and reduced-motion clock. Differences in cells and pixels stay within
the path row or four preview rows. Input sequences, intents, cursor and terminal
modes are unchanged. `scrollback.text` equals the visible text on both sides;
its line count stays zero and its buffer length stays unchanged. Browser
`renderCount` differs by zero, one or two asynchronous renders, recorded per pair.
These fixed-frame captures make no animation-cadence or latency claim. The wide
preview pair was visually inspected, as was the earlier narrow filename pair.

This slice has no new resource, PTY or end-to-end latency measurement. Existing
whole-rewrite performance and implementation-removal requirements still apply.

## Reproduction

```sh
HARNESS_TUI_PLAN_FRAMES=/tmp/tui-plan-geometry-final HARNESS_TUI_REFERENCE_FRAMES=/tmp/tui-plan-geometry-matrix cargo nextest run --profile ci -p harness-tui --all-features --lib --test rewrite_reference_test -E 'binary(rewrite_reference_test) | test(terminal_wrap_counts_display_rows_before_scroll) | test(shell_panel_wraps_operators_without_splitting_quoted_or_heredoc_payload)'
HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/tui-plan-geometry-journeys cargo nextest run --profile ci -p harness-tui --all-features -j1
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
node scripts/qa/render-recorded-frames.mjs /tmp/tui-plan-geometry-reference .omo/evidence/tui-rewrite/plan-geometry/reference --source-root /home/urbanbreach/.codex/worktrees/tui-reference/agent-harness
node scripts/qa/render-recorded-frames.mjs /tmp/tui-plan-geometry-final .omo/evidence/tui-rewrite/plan-geometry/candidate
```

The browser input directories contain the corresponding `frames/*.ansi.gz`
files decompressed. Their producer metadata is retained in the browser manifests.
To rerecord the original, restore the four files under `reference-fixture` into
its tests directory and run the plan journey there with
`HARNESS_TUI_PLAN_RECORD_REFERENCE=1`; this mode checks the pinned production
sources before recording the known defects. The original recorder's existing
settled-paint allowance is preserved. All eight original frames and their
pre-change candidate counterparts are retained as compressed complete records.
`source.json`, `source.patch.gz`, `binaries.json` and `files.json` bind the final
source, tested executables and published evidence.

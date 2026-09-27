# Selection layout replacement

Based on `0b041626`. `WrappedText` stores graphemes in one vector and indexes each row by a range. The hand-written segmenter and separate keyboard module are removed. Selection, copying and navigation retain the public API, explicit empty rows, source newlines, soft-wrap whitespace rules and inclusive cell endpoints. Row text is copied from the original source slice.

Viewer layout uses the already measured row count. Opening builds the display once, and height-only resizing no longer wraps the same text again. Search obtains byte boundaries directly from the installed Unicode segmenter instead of building a selection layout and scanning it for every cell. No backend contract, dependency or terminal setup changes.

## Unicode correction and review

R11 records the original segmenter's incomplete Unicode handling. The existing selection test demonstrates a decomposed Hangul syllable split across two rows at width two; the existing viewer search test demonstrates a match inside that syllable. Both fail on the pinned original and on the candidate before replacement. The final implementation uses Unicode grapheme segmentation and the same cluster-width measurement as viewer painting, retaining the existing minimum-one-cell rule.

Independent source review caught an interim regression: preserving the old maximum-character width undercounted a spacing-mark grapheme. The viewer fixture now searches the `X` in `कःX`, checks its painted reverse-video highlight, and copies exactly `X` at cell two. That check fails on the interim candidate, passes on the final candidate, and passes on the original before its later Hangul assertion fails. The defect's red logs are retained; the interim implementation is not delivered.

The source change removes 188 lines and two files. The source tree contains 557 Rust files and 172,991 lines, including inline tests. Integration tests contain 117 files and 27,398 lines. The replacement selection file is 262 lines; the touched viewer-state file falls from 521 to 500 lines. The whole rewrite remains far from its source-reduction target.

## Verification

The 20 focused checks and full 1,738-test run pass, with six skips in the full run. After flattening a nested wrap guard for Clippy, the final 21-test run repeats selection, viewer and the frozen matrix, followed by the production viewer journey. All 539 buffers equal the preceding approved candidate, retaining its existing R8 corrections. Clippy, workspace check, formatting and test-suite gates pass.

All 51 production viewer ANSI frames equal the pinned original byte for byte. Two representative frames—wrapped selection at 40×32 and search at 120×40—are replayed in the same xterm/browser/font setup. Their PNG bytes and every terminal snapshot field match exactly; both candidate images were visually inspected. `viewer/frames` stores the shared ANSI bytes once, and `viewer/comparison.json` records both hashes for every frame. `binaries.json` identifies the debug test executables that produced the frames.

The debug viewer journey took 8.20 seconds on the original and 13.13 seconds on the final candidate in separate scoped runs. This is a diagnostic concern, not a controlled release comparison; the next viewer work requires a release workload to establish its cause. No resource win is claimed here.

```sh
cargo nextest run --profile ci -p harness-tui --all-features -j1 --test transcript_selection_test --test transcript_block_viewer_test
HARNESS_TUI_REFERENCE_FRAMES=/tmp/frames HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/viewer cargo nextest run --profile ci -p harness-tui --all-features -j1
HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/reference-viewer cargo nextest run --profile ci -p harness-tui --all-features -j1 --lib -E 'test(native_tool_viewer_capture_uses_the_enter_handler)'
node scripts/qa/render-recorded-frames.mjs INPUT .omo/evidence/tui-rewrite/selection-layout/SIDE
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
```

The viewer captures drive the production `AppState` Enter and key handlers, then encode a terminal frame. Browser replay checks xterm output; it is not a PTY lifecycle or end-to-end latency measurement. This slice makes no new resource-performance claim. The preceding [release measurements](../settled-projection/README.md) and unchanged acceptance limits remain in force, including their unmet targets. No live-provider verification is added.

Independent source and evidence review approved this slice for commit; `review.txt` records the finding and its resolution. Whole-rewrite acceptance remains incomplete.

# Visible viewer rows

Based on `a44e1445`. Production viewer frames materialize only the visible row
window. Search and selection use absolute row positions; scrollbar length and
filter width still use the full content length. The public owned surface API
continues to expose every row through the same projector and painter. Glyph spans
borrow the visible text instead of allocating a string for each grapheme.

This bounds row materialization; viewer reflow, state and the rest of the TUI
still require replacement. Production source grows by 16 lines, and an existing
behavioral test grows by 49 lines. The source tree contains 557 Rust files and
173,056 lines, including inline tests. Integration tests contain 118 files and
27,661 lines. No backend contract or dependency changes.

## Release resources

The unchanged [viewer fixture and limits](../viewer-baseline/README.md) use 2,000
lines, 200 measured frames, three serial timing runs and a separate allocation
trace. These are public input/preparation/paint/diff/ANSI costs, not end-to-end
terminal latency. Forced redraw is not runtime idle. Values are medians of three
runs except allocated bytes, which come from the separate whole-process trace.

| Workload | p95 µs, before / candidate | p99 µs | CPU ms/frame | RSS KiB | Allocated bytes |
|---|---:|---:|---:|---:|---:|
| Forced redraw | 1834 / 1252 | 1942 / 1258 | 1.80 / 1.25 | 59492 / 59724 | 371551807 / 197909070 |
| Scroll | 1928 / 1454 | 1958 / 1524 | 1.85 / 1.35 | 59772 / 60604 | 374846891 / 201225098 |
| Search | 8009 / 7563 | 8827 / 7751 | 7.70 / 7.40 | 59680 / 60288 | 847017594 / 673228358 |
| Resize | 48020 / 46441 | 49511 / 48573 | 35.60 / 35.30 | 81548 / 83092 | 5858969857 / 5645242885 |

Every workload preserves output bytes, final screen, tail and interaction
checkpoints in every repetition. All frozen viewer limits pass except resize
RSS: 83,092 KiB exceeds 63,470.4 KiB. This slice does not improve retained memory.
Resize preparation remains the dominant cost at a median 35,445.5 µs. The
whole-rewrite limits remain separately binding and unmet.

`comparison.json` includes the pinned original, preceding candidate and this
candidate, phase medians and acceptance results. `release` contains all raw
samples/logs; `release-binary.json` identifies the executable and unchanged
fixture/driver hashes. Compilation and browser capture finished before timing.

## Verification

All 1,738 TUI tests pass, with seven skips. All 539 recorded cell/cursor/intent
frames equal the preceding approved candidate, including its documented R8
corrections. All 51 viewer ANSI frames equal the pinned original. The comparison
receipts reference the already published identical data instead of duplicating
it. `source.json`, `source.patch` and `debug-binary.json` identify this candidate.

Independent review requested one missing window-boundary check. The existing
viewer journey now selects 51 lines using application keys, scrolls until both
selection endpoints are offscreen, verifies copied content and compares every
popup cell with the full-surface API. A deliberate mutation that discarded
selections starting above the window fails on the painted background. Restored
source passes. This is a mutation check, not a newly discovered original defect.
After extracting the added scenario to satisfy Clippy's complexity limit, the
final journey, Clippy, formatting and suite gates pass. The production source is
unchanged from the full suite run; the workspace check also passes.

Two representative frames were replayed in the same xterm/browser/font setup:
wrapped selection at 40×32 and search at 120×40. Both PNGs and every display/cell
field match the original, and both images were visually inspected. The search
capture's browser `renderCount` is four instead of three. This callback count is
retained in `browser/comparison.json`; it changes neither ANSI nor pixels and is
not an application animation or latency measurement.

```sh
HARNESS_TUI_REFERENCE_FRAMES=/tmp/tui-viewer-window-frames HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/tui-viewer-window-journeys cargo nextest run --profile ci -p harness-tui --all-features -j1
HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/tui-viewer-window-journeys cargo nextest run --profile ci -p harness-tui --all-features -j1 --lib -E 'test(native_tool_viewer_capture_uses_the_enter_handler)'
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
node scripts/qa/render-recorded-frames.mjs /tmp/tui-viewer-window-xterm .omo/evidence/tui-rewrite/viewer-window
python3 scripts/measure-tui-rewrite.py --viewer-lines 2000 --output /tmp/tui-viewer-measurements/window --allocations
```

For browser replay, the input directory contains the two named `.ansi` files
from the journey and a `producer.json` describing the AppState/Crossterm entry
point and static fixture clock. This slice adds no PTY lifecycle or live-provider
verification. Whole-rewrite acceptance remains incomplete.

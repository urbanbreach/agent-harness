# Compact text geometry

Based on `c5b3f811`. The viewer and dashboard now keep one source string,
compact byte/cell ends and display-row ranges. Selection, copying and navigation
borrow grapheme slices. The public owned `WrappedText` inspection API keeps its
contract through the same geometry engine. Viewer search no longer retains a
second display string. Painting measures each style span once per row.

The first replacement stored only row ranges. Independent review found that
mapping many matches on a long unwrapped line repeatedly scanned the entire
line, including when that row was offscreen. Retaining compact grapheme ends
and using binary search removes that regression. The index costs 16 bytes per
grapheme on this host, without individual text allocations.

Production source grows by 84 lines; behavioral tests grow by 85 lines. The TUI
source tree contains 558 Rust files and 173,140 lines, including inline tests;
integration tests contain 118 files and 27,746 lines. The new geometry engine is
287 lines. This is a migration slice: the remaining state engine and formatters
still require replacement. No dependency, coordinator or backend changes.

## Release resources

The unchanged [viewer fixture and limits](../viewer-baseline/README.md) use
2,000 lines, 200 measured frames, three serial timing runs and a separate
whole-process allocation trace. These measure public input, preparation, paint,
diff and ANSI encoding; they are not end-to-end terminal latency. Forced redraw
is not runtime idle. Compilation and browser capture finished before timing.

| Workload | p95 µs, before / candidate | p99 µs | CPU ms/frame | RSS KiB | Allocated bytes |
|---|---:|---:|---:|---:|---:|
| Forced redraw | 1252 / 636 | 1258 / 647 | 1.25 / 0.6 | 59724 / 45148 | 197909070 / 167771861 |
| Scroll | 1454 / 657 | 1524 / 664 | 1.35 / 0.65 | 60604 / 36008 | 201225098 / 171081345 |
| Search | 7563 / 6694 | 7751 / 6986 | 7.4 / 6.6 | 60288 / 44884 | 673228358 / 643088949 |
| Resize | 46441 / 32777 | 48573 / 34065 | 35.3 / 21.55 | 83092 / 44784 | 5645242885 / 2450002890 |

The preceding candidate is `c5b3f811`. All frozen viewer limits now pass,
including resize RSS (44,784 KiB against 63,470.4 KiB). Resize preparation still
dominates at a median 20,553 µs. Every repetition preserves output bytes, final
screen, tail and scroll/search/reflow checkpoints against both the preceding
candidate and pinned original.

`comparison.json` includes original, preceding and current results, phase
medians and unchanged limits. Phase values are medians of three per-run medians;
allocated bytes and heap peaks come from separate traces. `release` contains
all raw samples and logs. Whole-rewrite limits remain separately binding and
unmet; these viewer results do not establish overall completion.

## Verification

All 1,739 TUI tests pass, with seven skips. Scoped all-target/all-feature Clippy,
the workspace check, formatting and suite gates pass. All 539 cell/cursor/intent
frames equal the preceding approved candidate, including its documented R8
corrections. All 51 viewer ANSI frames equal the pinned original. Receipts point
to the previously published identical data.

An existing selection test adds empty rows, CJK, combining text, wrap-triggering
whitespace and exact copy/byte positions; it passed on the preceding source.
One new public AppState journey opens a 20,000-character line followed by short
rows, disables wrapping, searches for the repeated character and reaches the
tail. The row-scanning attempt times out at the existing 20-second test deadline;
the preceding commit passes in 0.532 seconds and the indexed replacement passes.
The failing source, test and logs remain under `regression` and `offscreen-red.log`.
This is a regression against the preceding candidate, not a newly established
defect in the pinned original. The green run covers 21 focused checks.

Two representative frames were replayed through the same xterm/browser/font:
wrapped selection at 40×32 and search at 120×40. Both PNGs are byte-identical to
the originals and were visually inspected. Every cell/display field matches.
Search has four browser render callbacks instead of three; this incidental
snapshot field is retained and is not an application cadence or latency claim.
No new PTY lifecycle or live-provider verification is claimed.

## Reproduction

```sh
HARNESS_TUI_REFERENCE_FRAMES=/tmp/tui-compact-layout-frames HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/tui-compact-layout-journeys cargo nextest run --profile ci -p harness-tui --all-features -j1
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
node scripts/qa/render-recorded-frames.mjs /tmp/tui-compact-layout-xterm .omo/evidence/tui-rewrite/compact-layout
python3 scripts/measure-tui-rewrite.py --viewer-lines 2000 --output /tmp/tui-viewer-measurements/compact --allocations
```

The xterm input directory contains `runtime-viewer-long-select-40x32-motion-0ms.ansi`
and `runtime-viewer-search-120x40-motion-0ms.ansi` from the journey, plus a
`producer.json` identifying AppState/Crossterm and the static fixture clock.
`source.patch` and `source.json` identify the final source and tests. Binary
receipts identify the tested executables and unchanged performance fixture.

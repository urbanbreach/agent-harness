# Borrowed transcript painting

The transcript painter now reads prepared spans directly into the frame buffer.
It replaces visible-row/String cloning, mutable animation copies, temporary
Paragraph/rail buffers and duplicate background style passes. Marker colors and
spinner glyphs are selected while painting; prepared state remains immutable.
The backend and public TUI contracts are unchanged.

The painter is 317 lines. The 445-line `rows.rs` retains the existing row
formatters with unchanged behavior; moving them does not count as replacing
those formatters. One 164-line test file replaces five older checks with a
behavioral table that covers painted motion, label stability and terminal widths.
The change removes 20 production and 55 test lines. Whole TUI source is 167,968
lines across 560 Rust files, including tests. The 7.65% reduction from 181,882
original lines remains far short of the whole-rewrite requirement.

## Correctness and appearance

All 1,666 deterministic TUI tests pass, with seven configured skips. TUI Clippy
with all targets/features, workspace check, formatting and suite gates pass.
The 555 complete reference-matrix records and ANSI outputs match the preceding
candidate. Existing exact-clock chat/tool fixtures add 733 matching ANSI and
text captures, including normal/reduced motion and tool lifecycle states.

Six paired xterm.js screenshots at 0, 330 and 660 ms are byte-identical. They
show reasoning and expanded running-tool rows at 120×40. Cell records, text and
styles agree; asynchronous render counters differ. These replay exact-clock
production output through the same emulator/font/browser. They do not measure
runtime animation scheduling or browser-visible response latency.

Seven tests in the gated P0-03/P1-04 binaries pass. P1-04 runs six real TUI
sessions across three sizes and Unicode/ASCII modes, including detach and resize
bursts. Its raw archive contains all captures and receipts; every child exited
and PTY closed. P0-03 exercises streaming Markdown and final output through its
PTY owner. These fixtures use reduced motion.

The temporary painter diagnostic compares seeded terminal buffers, split and
whole spans, controls, alignment, zero/narrow/clipped dimensions, scroll offsets,
rail overlays and motion phases. Its final 10,788 before/candidate records are
byte-identical. `paint/records.json.gz` is their shared raw output, with separate
before/candidate hashes in the comparison receipt. The fixture is archived for
reproduction and is absent from the maintained test suite.

Independent source review found a missed terminal-width case in the first
replacement: UnicodeWidthStr undercounts halfwidth dakuten/handakuten relative
to Ratatui's CellWidth. `ｶﾞX` lost the cell containing `X`. The existing paint
check was extended with both marks, whole/split spans and three alignments. It
failed on that replacement, then passed on the preceding painter and the fixed
replacement. Text and rail clipping now use Ratatui's width API. The expanded
diagnostic also includes halfwidth text and rails. A separate mutation that
froze animation phase fails the maintained motion check. Both failures are
archived under `checks/`.

The initial resource measurements used the rejected width implementation. They
are retained under `rejected-width/` and excluded from acceptance evidence.

## Resource measurements

The final set contains 54 timing runs and 18 separate allocation runs. Each
workload has 200 measured frames after ten warmups. Timing/resource values are
medians of three runs; allocation totals come from one separate `memusage` run
per workload/build. The serial runner interleaves original, preceding and final
executables, reversing order on repetition two. Compilation, browser capture
and large evidence processing finished before measurement.

The original is `1bb0f989`, the preceding source is `db0e5b67`, and the final
candidate is that commit plus `source.patch.gz`. The preceding retained binary
is the final compact-selection executable; its hash agrees with that slice's
published receipt. The benchmark and shared journey source are unchanged.

| Workload and metric | Original | Preceding | Candidate |
| --- | ---: | ---: | ---: |
| Stream p95 / p99, µs | 3,975 / 4,260 | 1,063 / 1,098 | 968 / 998 |
| Stream CPU, ms/frame | 2.15 | 0.7 | 0.65 |
| Stream RSS, KiB | 49,016 | 33,276 | 33,296 |
| Stream allocated bytes | 744,456,856 | 324,778,212 | 312,383,317 |
| Resize p95 / p99, µs | 5,004 / 5,102 | 2,677 / 2,775 | 2,660 / 2,772 |
| Resize CPU, ms/frame | 2.8 | 0.95 | 0.95 |
| Resize RSS, KiB | 63,312 | 39,912 | 39,756 |
| Resize allocated bytes | 670,836,266 | 353,243,524 | 348,823,496 |

Streaming p99 falls 9.1% and allocation totals 3.8% against the preceding
build. Resize p99 changes by −0.1%, while allocations fall 1.3%. Static p99
values are 173, 133, 140 and 169 µs for startup, idle, typing and scrolling.
All fourteen unchanged frozen main-workload checks pass in this run.

RSS changes stay within 0.5% of the preceding build across these workloads;
streaming RSS is 20 KiB higher. Streaming peak heap falls 0.24%; other heap
changes are small. This slice removes transient paint allocations and improves
frame cost, with no substantial retained-memory improvement demonstrated.
Streaming/resize CPU, allocation totals and RSS remain more than 30% below the
original. CPU ticks are coarse for short static workloads.

Output bytes, oldest-history content and workload diagnostics match the
preceding build in every timing and allocation run. Original scrolling differs
only by the previously accepted R8 viewport correction. No content was dropped.

These measurements include public handlers, preparation, painting, Ratatui
buffer diffing and Crossterm encoding to a counting sink. They do not measure
sustained runtime CPU or end-to-end terminal latency. The unchanged fixed-
viewport resize fixture queries terminal size and falls back to `tput` without
a terminal; process CPU excludes child CPU. The trace documenting this limit is
in `../compact-selection/diagnostics/size-trace`.

## Reproduction and remaining work

`source.json` and `source.patch.gz` identify the exact source. Executable
receipts, all raw samples/logs, medians and exact commands are under
`performance/`. The runner records each command and start time and uses nextest
build reuse. Build each recorded source before running the serial measurements:

```sh
cargo nextest list --list-type binaries-only --release -p harness-tui \
  --all-features --test rewrite_performance_test --message-format json
cargo metadata --format-version 1
```

Retain each executable and update the saved `performance/measure.py` local paths
when reproducing on another host. Keep allocation measurements separate.
`rejected-width/` preserves the excluded first run and its source patch.

The existing exact-clock fixtures can be captured with:

```sh
HARNESS_PARITY_RENDER_ARTIFACT_DIR=/tmp/painter-frames \
  cargo nextest run --profile ci -p harness-tui --all-features \
  --test grok_parity_render_test \
  -E 'test(chat_and_tool_bullets_animate_without_recoloring_labels_or_reflowing_text) | test(all_tool_families_keep_grok_columns_through_streaming_and_disclosure)'
node scripts/qa/render-recorded-frames.mjs \
  /tmp/painter-selected-frames .omo/evidence/painter-selected
```

The six selected frames and producer metadata are identified by the browser
manifests. Their `source` field describes the replay process's checkout; the
`producerMetadata` field identifies the renderer that created the input bytes.
The temporary diagnostic can be reproduced by adding its archived `fixture.rs`
as a test-only child of `ui_transcript_surface`, selecting
`test(diagnostic_painter_parity)` with nextest, and setting
`TUI_SURFACE_PAINT_OUT` to a temporary JSON file. Remove that module afterward.

Independent review approved the corrected implementation and final evidence;
the report is in `review.txt`. Remaining state/formatter replacement, source reduction, sustained
runtime and final whole-rewrite signoff are still open. No live provider or new
platform verification is claimed.

# Borrowed styled wrapping

Based on `dafd4ed9`. A 234-line replacement borrows word tokens and source link
clusters, allocating the final rows. The old token vector, per-cluster link
strings, duplicate long-token paths and duplicate clipping helper are removed.
Tab expansion uses the existing display-width helper, preserving newline
accounting and four-column stops across styles. Hyphen breaks, leading-space
rules, preformatted behavior and link coalescing remain intact.

Selection now measures graphemes with the same installed Unicode-width library
as Ratatui. The existing nested-link journey checks copy at painted coordinates
with VS16 before and inside a styled link, and checks highlight while dragging.
Production code shrinks by 54 lines and inline tests by seven; integration
fixtures add 55 lines. The source tree has 558 Rust files / 169,189 lines including
inline tests. Composer measurement is outside this change. No dependencies or backend
contracts change. The remaining surface painter and state engine are still legacy.

## R14: transcript Unicode geometry

The original and preceding candidate lose clusters when a long token contains
VS16 emoji-presentation sequences. At widths 40, 80, 120 and 160, they paint
29/40, 55/80, 82/120 and 109/160 clusters. The replacement paints all of them.
The old tokenizer also splits a space from its combining mark and drops that
space. These are recorded defects; the decision is to preserve whole graphemes
and use terminal-cell widths.

The existing long-token test now checks behavior in a table rather than an
implementation fast path against a duplicate algorithm. During review, its
zero-width-prefix cases exposed extra rows both inside a token and across styled
spans. Both row-flush guards now require occupied cells. The public link journey
also exposed the inherited selection width mismatch: copying the painted label
returned `old #️link` instead of `bold #️link`. Selection measurement is corrected
at its shared extraction and row-projection boundaries.

The red logs retain the observed failures. `styled-prefix-red.log` also contains
a test mistake: its initial highlight assertion ran after mouse-up had correctly
auto-copied and cleared selection. The final check asserts highlight during drag.
That failure is not evidence of a production highlight defect. `module-path.log`
and `clippy.log` record ordinary compile/lint fixes, not behavior failures. The
unchanged animation tests moved to the parent file's end for the lint policy.

## Verification

The final serial run passes 1,682 tests with seven configured skips. Seven gated
PTY checks, all-target/all-feature TUI Clippy, workspace check, formatting and
suite gates pass. The 543 main records and eight plan records exactly match the
preceding candidate, including cells, styles, cursor, inputs and intents.

Four new public-journey records match the original before the change. Their
post-change cell and xterm pixel differences stay within reply rows. Both Ratatui
and xterm retain every expected cluster; cursor and terminal modes match.
Asynchronous xterm render counts differ at widths 80 (3→4) and 120 (3→5).
The 40-column candidate screenshot was visually inspected. These are fixed-frame
replays, not new cadence or end-to-end terminal-latency measurements.

```sh
HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/tui-styled-wrap-journeys HARNESS_TUI_WRAP_FRAMES=/tmp/tui-styled-wrap-final HARNESS_TUI_REFERENCE_FRAMES=/tmp/tui-styled-wrap-matrix HARNESS_TUI_PLAN_FRAMES=/tmp/tui-styled-wrap-plans cargo nextest run --profile ci -p harness-tui --all-features -j1
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tui --all-features --run-ignored all --test p0_01_pty_recorded --test p0_02_pty_recorded --test p0_03_pty_recorded -j1
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
node scripts/qa/render-recorded-frames.mjs /tmp/tui-styled-wrap-xterm/reference .omo/evidence/tui-rewrite/styled-wrap/reference --source-root /home/urbanbreach/.codex/worktrees/tui-reference/agent-harness
node scripts/qa/render-recorded-frames.mjs /tmp/tui-styled-wrap-xterm/candidate .omo/evidence/tui-rewrite/styled-wrap/candidate
```

For original defect capture, run `wrapping_geometry` with
`HARNESS_TUI_WRAP_RECORD_REFERENCE=1` and `HARNESS_TUI_WRAP_FRAMES` set. The
record-only mode validates the pinned production tree. Exact original test
instrumentation is archived under `reference-fixture`; candidate source and its
patch are bound by `source.json`. Browser inputs are the corresponding ANSI
files decompressed, with producer metadata from the manifests.

## Release measurements

The original and candidate use the identical public benchmark, release mode and
all features. Builds, tests and browser captures finished before these serial
runs: three runs of 200 measured frames per workload, plus one separate glibc
`memusage` run. Non-startup workloads retain 1,000 turns / 4,000 events. The
before set reuses the previous slice's measured candidate, whose source is now
committed as `dafd4ed9`; it is an earlier sequential measurement, not an
interleaved comparison. Raw samples, allocation logs and executable receipts
for all three sets are preserved.

```sh
cargo nextest list --release -p harness-tui --all-features --test rewrite_performance_test --message-format json
python3 scripts/measure-tui-rewrite.py --root REFERENCE --output /tmp/tui-styled-wrap-reference-perf --allocations
python3 scripts/measure-tui-rewrite.py --output /tmp/tui-styled-wrap-candidate-perf --allocations
```

| Workload | p95 original / candidate (µs) | p99 original / candidate (µs) | CPU ms/frame original / candidate | RSS KiB original / candidate | Allocated bytes original / candidate |
|---|---:|---:|---:|---:|---:|
| stream-1000 | 8,795 / 2,617 | 9,294 / 2,709 | 4.75 / 1.70 | 51,816 / 35,688 | 744,459,345 / 351,557,394 |
| resize-1000 | 9,613 / 5,586 | 10,099 / 5,921 | 5.60 / 2.05 | 65,804 / 42,232 | 670,823,099 / 358,480,344 |

Against the preceding candidate, streaming allocation falls 7.0% (378,157,979→
351,557,394 bytes) and resize allocation falls 1.7% (364,755,885→358,480,344).
Startup allocation falls 22.4%; the other three workloads fall about 1.5%.
Peak heap falls 0.1–1.1%; RSS is effectively unchanged. These totals include
construction, workload and diagnostic work. Instrumented allocation runs are
excluded from the timing medians.

Timing changes are mixed. Streaming p95 rises 2,555→2,617 µs and p99
2,639→2,709 µs; coarse CPU rises 1.65→1.70 ms/frame. Resize p95 falls
5,684→5,586 µs and CPU 2.10→2.05 ms/frame, while p99 rises 5,890→5,921 µs.
Startup p99 rises 435→461 µs; scrolling falls 435→405 µs. Cold-frame time
rises in every before/candidate workload, by 4.7–9.9%. Full construction, cold,
percentile, CPU and memory comparisons are published rather than attributing
all variation to this change. CPU tick resolution is 0.05 ms/frame.

Against the paired original, accumulated rewrite gains remain substantial, but
startup p99 rises 408→461 µs. All four static p99 workloads remain within the
paired allowance of the larger of 100 µs or 10%. The earliest frozen acceptance
limits remain unchanged: streaming p95/p99, both RSS limits and both allocation
limits pass; resize timing, all four static p99 limits and both CPU limits fail.
The earlier host timing shift remains unexplained. Source reduction, the
remaining implementation replacement and sustained-runtime targets are unmet.

All timed byte counts and oldest retained screens match. Every before/candidate
post-timing diagnostic matches too. The original/candidate scrolling diagnostic
retains the documented R8 blank-gap correction. The measurement boundary is
input/event handling, preparation, rendering, diff and ANSI encoding to a
counting sink. This slice makes no fresh live-provider, sustained-idle-runtime
or end-to-end terminal-latency claim.

Independent source and evidence review approved this slice for commit.
`review.txt` records the resolved findings and verification scope. `files.json`
binds the published artifacts. Whole-rewrite completion remains open.

# Compact selection rows

This slice replaces the per-cell `Vec<String>` selection model with one row
string, display-cell bounds, and copy/link metadata. Semantic rows move into the
prepared layout without another join or clone. The old row type, converter and
cell-array representation test are removed. Copy extraction, highlight painting,
semantic anchors and the distinction between semantic and fallback rows remain.
The backend and public contracts are unchanged.

The change removes 52 production lines and adds six test lines overall. The
replacement files are 413, 372 and 161 lines; retained focused tests occupy a
279-line file. Whole TUI source is 168,043 lines across 558 Rust files, including
tests, versus 181,882 lines in the original. The 7.61% reduction remains short of
the whole-rewrite requirement.

## Correctness and appearance

- All 1,670 deterministic TUI tests pass, with seven configured skips. TUI Clippy
  with all targets/features, workspace check, formatting and suite gates pass.
- Seven gated PTY checks cover dashboard return, detached navigation, resizing,
  Markdown and terminal cleanup.
- All 555 complete frame records and their ANSI outputs equal the preceding
  candidate. `matrix.json` links the unchanged raw records; the compressed ANSI
  comparison records each hash.
- A temporary diagnostic compared 3,136 previous/candidate row records across
  narrow widths, alignment, rails, controls, combining marks, wide/ZWJ/VS16
  graphemes, and span boundaries. Every record is byte-identical. The diagnostic
  fixture and results are under `rows/`; it is not a maintained duplicate engine
  or an added production test.
- The existing mouse-copy journey now selects wrapped Unicode and a link, resizes
  40 → 140 → 40 columns, checks the painted highlight, and copies exact text and
  its destination. It passes before and after replacement. Deliberately bypassing
  semantic anchor resolution produces the retained failure in `checks/red.log`.
  The mutation is absent from the implementation.
- Fresh reference/candidate xterm.js captures have identical text. The before
  PNGs and cells match. After selection, only foreground/background colors of
  21 cells at row 15, columns 5–25 differ, reproducing the already accepted R7
  correction. Async parser counters also differ. Both processes restore
  termios and terminal modes, exit successfully, and remove their process groups,
  sockets and browser profiles. The final candidate includes the capacity trim.

## Resource measurements

The final set contains 54 timing runs and 18 separate allocation runs. Each
workload uses 200 measured frames after ten warmup frames; table timings and
process resources are medians of three runs. Runs interleave the original,
preceding and candidate executables, reversing the order on repetition two.
Allocation totals come from one separate `memusage` run per workload/build.

The original is `1bb0f989`; the preceding retained build is `8e7a3ca0`, whose
receipt is also published in `../styled-wrap`. The intervening `8b01eed0`
commit removed unreachable presentation paths. The candidate is `8b01eed0`
plus `source.patch.gz`. SHA-256 receipts identify every retained executable.
The benchmark and shared journey source are unchanged across builds.

| Workload and metric | Original | Preceding | Candidate |
| --- | ---: | ---: | ---: |
| Stream p95 / p99, µs | 4,033 / 4,277 | 1,209 / 1,242 | 1,068 / 1,102 |
| Stream CPU, ms/frame | 2.15 | 0.75 | 0.70 |
| Stream RSS, KiB | 49,132 | 33,136 | 33,360 |
| Stream allocated bytes | 744,446,946 | 351,563,526 | 324,787,449 |
| Resize p95 / p99, µs | 5,014 / 5,092 | 2,652 / 2,715 | 2,676 / 2,769 |
| Resize CPU, ms/frame | 2.80 | 0.95 | 0.95 |
| Resize RSS, KiB | 63,376 | 39,524 | 39,880 |
| Resize allocated bytes | 670,819,748 | 358,483,884 | 353,246,457 |

Streaming p99 falls 11.3% and allocations 7.6% versus the preceding build.
Resize p99 rises 2.0%, while allocations fall 1.5%. Static candidate p99 values
are 170, 149, 157 and 184 µs for startup, idle, typing and scrolling. All remain
within the paired-original static allowance. Long-history cold frames improve
16–19%; construction timing is mixed.

The first replacement retained excess vector capacity and raised peak heap by
3.9–6.0%. `shrink_to_fit()` at layout construction removes most of that increase.
The full first run and its patch are retained under `untrimmed/`. Final peak heap
is still 0.5–0.8% above the preceding build, and long-history RSS is 0.5–1.3%
higher. Those are regressions, although streaming and resize RSS remain more
than 30% below the original. No memory gain over the preceding build is claimed.

All fourteen frozen main-workload timing/resource checks pass in this run. The
limits were not changed. Both retained executables ran roughly twice as fast as
in the earlier styled-wrap timing set; their bytes are unchanged and the cause
is not established. Attribute this slice's improvement to the fresh paired
comparison, not the entire difference from earlier recordings.

Output bytes, oldest-history content and workload diagnostics match the
preceding build in every run. Original scrolling differs only by the previously
recorded R8 viewport correction. No content or history was removed.

These are public handler, frame preparation, renderer, Ratatui diff and Crossterm
encoding measurements to a counting sink. They do not measure sustained runtime
idle CPU or end-to-end terminal latency. The resize fixture uses a fixed Ratatui
viewport. A separate trace observed 420 `tput` calls during 210 resizes because
Crossterm's size query lacked a terminal. Process CPU counters exclude child CPU.
That diagnostic is excluded from acceptance samples and the fixture is unchanged.

## Reproduction and provenance

`source.json`, `source.patch.gz` and `source-counts.json` identify the change.
`performance/measure.py` records exact retained-binary paths and the serial
nextest runner. Its compressed `run-order.json` records every command and start
time; side directories contain raw samples, logs, binary receipts and medians.
`reference-comparison.json`, `before-comparison.json` and the two `frozen-*`
comparisons expose the calculations. The same layout exists for the untrimmed run.

Build the unchanged benchmark at each recorded source revision before measuring:

```sh
cargo nextest list --list-type binaries-only --release -p harness-tui \
  --all-features --test rewrite_performance_test --message-format json
cargo metadata --format-version 1
```

Retain the resulting executable, update the runner's local metadata/output paths
if reproducing on another machine, and run the saved `measure.py`. The runner
uses nextest build reuse, so compilation is finished before the timing samples.
Keep allocation runs separate. The browser command is:

```sh
cargo build -p harness-tui --all-features --example rewrite_probe
node scripts/qa/capture-rewrite-selection.mjs \
  target/debug/examples/rewrite_probe .omo/evidence/tui-rewrite/compact-selection/candidate
```

Use the pinned original probe with `--reference` for the original R7 behavior.
The original probe is a release executable; the candidate probe is debug. These
captures establish appearance and restoration, not comparative performance.
Stage and preparation profiling scripts/patches/results are under `diagnostics/`.
They were temporary, restored exactly, and are excluded from acceptance metrics.

Independent review approved the source and capacity correction. Final evidence
review is recorded separately. The remaining renderer/state rewrite, source
reduction, sustained runtime targets and final whole-rewrite signoff are open.

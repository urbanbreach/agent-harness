# Borrowed settled projection

Based on `c9b366f3`. Settled presentation borrows the canonical transcript and run summary instead of cloning both. Inline child sessions still project their recorded event slice into a local value. Only the final compaction checkpoint is copied before whole-state mutation. Error returns, retention, legacy compaction fallback and transient assistant suffixes retain their previous order and behavior.

The private `EventDetailsCache` type only forwarded `Vec` operations. It is removed, with two test fixture assignments adjusted to use the vector directly. There are no backend, dependency, public-contract or rendering changes. Whole-history presentation rebuilding and the event-detail mirror still remain; this slice removes temporary copies, not those engines.

The change removes 38 source lines. The TUI source tree has 559 Rust files and 173,179 lines, including inline tests. Integration tests remain 117 files and 27,357 lines. The original source baseline is 181,882 lines, so the 50% reduction target remains far off.

## Validation

All five existing settlement checks pass: canonical content, correlated response/tool ordering, finish-boundary settlement, invalid history handling, and legacy compaction presentation. No new test was needed for the borrow or pass-through-wrapper removal. The full serial run passes 1,738 tests with six skips. All 539 frozen buffers exactly equal the preceding approved candidate, retaining only the existing R8 corrections. Seven gated PTY checks, scoped Clippy, workspace check, formatting and test-suite gates pass.

`source.json` hashes the 559 source files, runtime probe, unchanged public benchmark and shared journey. `source.patch` records the source change. The prior [borrowed-text browser evidence](../borrowed-text/README.md) remains the latest xterm comparison; no fresh browser or end-to-end-latency result is claimed for this data-ownership change.

```sh
cargo nextest run --profile ci -p harness-tui --all-features -j1 --test typed_runtime_event_settle_test
HARNESS_TUI_REFERENCE_FRAMES=/tmp/frames cargo nextest run --profile ci -p harness-tui --all-features -j1
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tui --all-features --run-ignored all --test p0_01_pty_recorded --test p0_02_pty_recorded --test p0_03_pty_recorded -j1
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
```

## Release measurements

Both builds use `--release --all-features` and identical public benchmark fixtures. Each workload has three serial runs of 200 measured frames and one separate glibc `memusage` run. Non-startup workloads retain 1,000 history turns. Timings include event/input handling, frame preparation, paint, diff and ANSI encoding. Allocation totals include construction. These are not end-to-end terminal timings.

```sh
cargo nextest list --release -p harness-tui --all-features --test rewrite_performance_test --message-format json
python3 scripts/measure-tui-rewrite.py --root REFERENCE --output /tmp/reference --allocations
python3 scripts/measure-tui-rewrite.py --output /tmp/candidate --allocations
```

| Workload | p95 reference / candidate (µs) | p99 reference / candidate (µs) | CPU ms/frame reference / candidate | RSS KiB reference / candidate | Allocated bytes reference / candidate |
|---|---:|---:|---:|---:|---:|
| stream-1000 | 8,919 / 2,547 | 9,348 / 2,644 | 4.95 / 1.70 | 51,496 / 39,624 | 744,456,332 / 378,160,857 |
| resize-1000 | 9,785 / 5,533 | 10,209 / 5,853 | 5.65 / 2.00 | 66,036 / 46,060 | 670,829,118 / 364,761,155 |

These are original/accumulated-rewrite comparisons. Streaming p95/p99 fall 71.4%/71.7%, CPU 65.7%, allocations 49.2%, and RSS 23.1%. Resize p95/p99 fall 43.5%/42.7%, CPU 64.6%, allocations 45.6%, and RSS 30.3%.

Against the preceding candidate run, total allocation falls about 2.1 MB in each history workload. Peak heap and RSS do not improve measurably. That comparison is between successive runs, not a paired ablation of this change. The retained event mirror and whole-history presentation still need replacement.

Other paired p99 values rise slightly: startup 430→438 µs, idle 315→327, typing 330→344, and scroll 383→400. These remain within the paired-reference allowance. Coarse frame CPU rises from 0.30 to 0.35 ms for idle and 0.35 to 0.40 ms for scrolling; typing stays at 0.30 ms. Startup cold preparation rises 9.8% and peak heap 0.11%. All six workloads, including these regressions, are recorded in `performance/comparison.json`.

All timed byte counts and oldest retained screens match. Only the scroll workload's extra post-timing screen differs, preserving the documented R8 blank gap. `binaries.json` identifies release benchmark executables, not the shipped CLI.

The earlier frozen absolute timing limits remain unchanged. Streaming now meets its frozen p95/p99 limits, but resize misses them. Startup, idle, typing and scrolling p99 also exceed their earliest absolute allowances, despite passing the paired-reference comparison. `performance/frozen-timing-comparison.json` records both values and limits. The host-wide timing change remains unexplained; no limits are reset. Streaming RSS, source reduction and sustained runtime typing/burst CPU targets are also unmet. No fresh browser-latency, runtime-idle or live-provider result is claimed.

Independent source and evidence review approved this migration slice for commit; `review.txt` records its scope. The whole rewrite remains incomplete.

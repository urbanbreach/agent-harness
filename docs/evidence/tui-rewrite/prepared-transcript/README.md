# Prepared transcript evidence

This is an intermediate migration review. The old composite state engine and surface renderer remain; whole-rewrite resource acceptance is open.

`source.json` identifies the candidate Rust files compiled for the final checks. The reference production source is pinned to `1bb0f98988670a5f4b48cdf749b455a79cfdaa82`. Both public performance builds use the same benchmark and journey fixture. The candidate is an uncommitted tree based on `f24e8d6c`; this commit contains its source changes. Binary hashes identify measurements, not a shipped release.

The final deterministic run uses `-j1`: 1,783 pass, six skip. Seven explicitly gated PTY checks and scoped all-target/all-feature Clippy pass. `earlier-parallel-timeout.log` retains a prior 1,782-pass run with one 20-second viewer-capture timeout, before compact padding was improved. No timeout or acceptance limit was changed. Do not describe the final serial result as a default-parallel pass.

## Reproduce functional checks

```sh
cargo nextest run --profile ci -p harness-tui --all-features -j1
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tui --all-features --run-ignored all --test p0_01_pty_recorded --test p0_02_pty_recorded --test p0_03_pty_recorded -j1
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
```

The selection red/green logs cover a drag below a sticky prompt, highlight placement after release, offscreen copy, and Escape. R8's preparation-red log fails because a second preparation changes scroll offset from 12 to 11 without input. The final oracle also requires repeated preparation to preserve the buffer. The existing Ctrl-Up/Down test crosses section gaps in both directions with width round trips.

`gap/comparison.json` accounts for every golden frame: 537 are exact; the two 40×24 detached frames retain one blank row. Their expected changes are derived from the original body cells. The frozen golden file is unchanged. `gap/scroll-*.json.gz` records the public 80-step trace before and after the correction. The corrected trace equals the reference first paints and is invariant under extra preparation; repeated reference paints stall at a gap.

To reproduce that diagnostic trace, copy `gap-scroll-trace.rs` to `crates/harness-tui/tests/rewrite_scroll_trace_test.rs` in each checkout (and use the shared `tests/support/rewrite_journey.rs`). Run:

```sh
HARNESS_REWRITE_TRACE_OUT=/tmp/scroll-trace.json cargo nextest run --profile ci -p harness-tui --all-features --test rewrite_scroll_trace_test
```

Remove the temporary test afterward. It records evidence; the maintained oracle and keyboard test provide the regression checks.

## Browser evidence

`selection/reference` and `selection/candidate` contain offline PTY/xterm.js runs with the same emulator, font, dimensions, theme, fixture, timestamps and reduced-motion setting. Before selection, every cell matches. After release, exactly 21 cells differ only in selection foreground/background. The reports include exit status, termios/protocol restoration, and process group/socket/browser-profile cleanup. These are behavior captures, not latency measurements. The reference probe is a release executable; the candidate probe is a debug executable, with hashes in their reports.

```sh
cargo build -p harness-tui --all-features --example rewrite_probe
node scripts/qa/capture-rewrite-selection.mjs REFERENCE/target/release/examples/rewrite_probe .omo/evidence/selection-reference --reference
node scripts/qa/capture-rewrite-selection.mjs target/debug/examples/rewrite_probe .omo/evidence/selection-candidate
```

`gap/reference` contains the pinned original screenshots; `gap/candidate` replays the corrected oracle's ANSI through the same xterm.js. These recorded-frame images are separate from the real PTY selection exercise.

## Release measurements

Complete compilation before collecting samples. Run each checkout serially, without concurrent builds or browser captures:

```sh
python3 scripts/measure-tui-rewrite.py --root REFERENCE --output /tmp/reference --allocations
python3 scripts/measure-tui-rewrite.py --output /tmp/candidate --allocations
```

Each public workload uses 1,000 history turns, 10 warm-up frames, 200 measured frames and three repetitions, plus a separate glibc `memusage` run. Startup has no history. Timers include public input/event handlers, frame preparation, paint, Ratatui diffing and Crossterm encoding to a counting sink. Allocation totals include fixture construction. These are not end-to-end terminal timings.

The internal selection fixture uses 100 drag/release/Escape cycles with four paints per cycle. Run `selection` with history 1,000 and `selection-long` with 1,000 and 10,000 paragraphs:

```sh
HARNESS_PERF_SCENARIO=selection HARNESS_PERF_HISTORY=1000 HARNESS_PERF_FRAMES=100 HARNESS_PERF_ARTIFACT_DIR=/tmp/selection cargo nextest run --release --profile perf -p harness-tui --all-features --lib -E 'test(perf_interactive_resources_under_load)' -j1 --success-output immediate
```

`selection-before` preserves the intermediate whole-history selection implementation and its single baseline sample. It is not the original reference. The longer single-block fixture checks that preparing fallback compaction rows once keeps later selection work bounded by the visible/selected rows. Retained content and cold preparation still scale with document size.

## Results and limits

Serial public medians on this host are:

| Workload | p95 reference / candidate | p99 reference / candidate | CPU ms/frame reference / candidate | RSS KiB reference / candidate | Allocated bytes reference / candidate |
|---|---:|---:|---:|---:|---:|
| stream-1000 | 8,827 / 8,764 µs | 9,333 / 9,279 µs | 4.75 / 4.2 | 51,460 / 46,416 | 744,460,862 / 511,276,394 |
| resize-1000 | 9,688 / 9,497 µs | 9,947 / 9,875 µs | 5.5 / 5.6 | 65,760 / 57,072 | 670,831,392 / 685,928,608 |

Streaming allocation volume is 31.3% lower; streaming RSS is 9.8% lower and resize RSS 13.2% lower. Resize allocations are 2.3% higher. Streaming/resize tail latency is roughly unchanged in this paired run and remains far above the earlier frozen timing targets. Cold preparation is also slower: streaming 55,627 → 69,347 µs and resize 55,820 → 69,491 µs. The owner prepares compact fallback selection rows up front; these costs must fall in the remaining renderer replacement. No performance acceptance threshold is relaxed.

All six scenarios preserve their reachable oldest text and encoded byte counts in every paired repetition. Five final diagnostic screens match exactly. Scrolling's extra `screen()` call after timing reproduces R8 in the reference by snapping off a two-row gap; the corrected candidate retains the gap. The initial timed scroll byte-count discrepancy led to R8 and is fixed in the final samples. Static renderer input/scroll p99 changes remain within the paired regression allowance. These short CPU-tick samples do not establish idle quiescence or sustained typing improvements; runtime evidence and final runtime signoff are separate.

The intermediate selection baseline was 11,293/11,348 µs p95/p99, 109 CPU ticks and 40,808 KiB RSS. The final four-paint selection sample is 1,842/1,916 µs, 19 ticks and 28,080 KiB. Single-block selection at 1,000 and 10,000 paragraphs is 1,680/1,754 µs and 1,450/1,472 µs p95/p99, with unchanged terminal output. Cold cost and retained memory still grow with the document. These single internal samples isolate selection; they do not replace the paired public workloads or final end-to-end latency signoff.

Source size remains 178,433 Rust lines under `src` including internal tests, versus the pinned 181,882. The required 50% reduction is not yet achieved. The new owner is 136 lines. Legacy code removal and the final production/test breakdown remain open.

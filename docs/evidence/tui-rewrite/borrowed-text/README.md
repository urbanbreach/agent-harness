# Borrowed text geometry

Based on `0fd863fa`. Rendering and selection iterate borrowed grapheme slices instead of allocating a vector and a string for every cluster. The existing segmentation and terminal-width rules remain unchanged. Output rows and link metadata still own text where their lifetimes require it. Composer editing retains its owned atoms.

`display_width` now sums Unicode widths over `str::lines`, matching the pinned Ratatui conversion, including CRLF and trailing-newline handling. Inline Markdown measures preceding spans only when a token has a link. Ordinary tokens no longer repeat that unused scan. Link validation, ranges and span coalescing are unchanged. There are no new caches, dependencies or backend changes.

## Functional evidence

`source.json` records all 559 Rust files under the TUI source tree, the runtime probe, the unchanged public benchmark and the shared journey. The source tree contains 173,217 lines, including inline tests; integration tests contain 27,357 lines in 117 files. Source reduction remains 4.8% against the original 181,882 lines. This optimization does not replace the remaining formatters.

The existing Unicode, wrapping, truncation, Markdown, safe/unsafe-link and selection checks cover the affected contracts. No new behavioral test or snapshot exception was added. The focused run passed 150 checks before the final Markdown guard; the final serial run passed all 1,738 tests with six skips. All 539 frozen checkpoints exactly equal the preceding approved candidate, including only the existing R8 corrections. Seven gated PTY checks, scoped Clippy, workspace check, formatting and test-suite gates pass.

The two controlled-timestamp xterm streaming captures match the original cells, text, cursor and PNG bytes. The real PTY selection capture matches the previous approved candidate before and after selection. It exits naturally with status zero, restores termios and terminal protocols, and removes the process group, sockets, browser profile and temporary directory. This debug-probe capture provides behavior evidence, not latency measurements.

```sh
HARNESS_TUI_REFERENCE_FRAMES=/tmp/frames cargo nextest run --profile ci -p harness-tui --all-features -j1
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tui --all-features --run-ignored all --test p0_01_pty_recorded --test p0_02_pty_recorded --test p0_03_pty_recorded -j1
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
# Copy stream-markdown-{40x24,120x40}-reduced-0ms.ansi from /tmp/frames into INPUT_DIR:
node scripts/qa/render-recorded-frames.mjs INPUT_DIR .omo/evidence/text-streaming
cargo build -p harness-tui --all-features --example rewrite_probe
node scripts/qa/capture-rewrite-selection.mjs target/debug/examples/rewrite_probe .omo/evidence/text-selection
```

## Release measurements

Both builds use `--release --all-features`. Compilation, tests and browser captures finish before sampling. Each workload has three serial runs of 200 measured frames plus a separate glibc `memusage` run. Non-startup workloads contain 1,000 history turns. Timings cover public input/event handling, preparation, paint, diff and ANSI encoding; allocation totals also include fixture construction. These are not end-to-end terminal latency measurements.

```sh
cargo nextest list --release -p harness-tui --all-features --test rewrite_performance_test --message-format json
python3 scripts/measure-tui-rewrite.py --root REFERENCE --output /tmp/reference --allocations
python3 scripts/measure-tui-rewrite.py --output /tmp/candidate --allocations
```

| Workload | p95 reference / candidate (µs) | p99 reference / candidate (µs) | CPU ms/frame reference / candidate | RSS KiB reference / candidate | Allocated bytes reference / candidate |
|---|---:|---:|---:|---:|---:|
| stream-1000 | 8,747 / 2,553 | 9,274 / 2,616 | 4.70 / 1.65 | 51,828 / 39,404 | 744,453,883 / 380,246,527 |
| resize-1000 | 9,722 / 5,589 | 10,223 / 5,879 | 5.60 / 2.00 | 65,916 / 45,728 | 670,832,693 / 366,841,985 |

These compare the original implementation with the accumulated rewrite. Streaming p95/p99 fall 70.8%/71.8%, CPU 64.9%, allocations 48.9%, and RSS 24.0%. Resize p95/p99 fall 42.5%/42.5%, CPU 64.3%, allocations 45.3%, and RSS 30.6%. Cold preparation falls 13–17% on the history workloads.

Typing p95/p99 rise from 314/319 to 325/331 µs; scrolling rises from 378/384 to 397/410 µs. These increases remain within the permitted allowance. Their coarse CPU medians are unchanged at 0.30 and 0.40 ms/frame. Startup and idle p99 fall from 579/343 to 451/317 µs. `performance/comparison.json` reports all six workloads, including small median-latency increases.

All timed byte counts and oldest retained screens match in every repetition. Only the scrolling workload's extra post-timing screen differs, retaining the documented R8 blank gap. No events or history were discarded. `binaries.json` identifies the release benchmark binaries; the debug probe has a separate receipt in its selection report.

Whole-rewrite acceptance remains unmet. Streaming RSS is short of the required 30% reduction. The original frozen absolute resize timing limits remain unchanged and are still missed, despite the improvement against this paired reference. Source reduction and sustained runtime typing/burst CPU targets also remain outstanding. These renderer samples establish neither end-to-end latency nor settled runtime-idle behavior; prior evidence retains those boundaries. Live providers and unsupported environments remain unverified.

Independent source and evidence review approved this slice for commit with no blocking findings; `review.txt` records its scope. This is not approval of the whole rewrite.

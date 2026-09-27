# Transcript outline replacement

The old transcript composite, event mirror, block store, invalidation/cache layers, lifecycle owner, and duplicated timeline navigation state are removed. `AppState` retains a compact outline containing turn identities, markers and block heights. Content stays in the activity projection. The viewer belongs to `AppState`; pager text and dashboard blocks are produced when requested. Existing viewer, pager, scroll and redaction contracts remain in use.

The outline retains the old timeline's nested viewport, initial navigation height, fold defaults, stable identities and return-anchor semantics. Its geometry is still separate from the prepared transcript surfaces. This is a migration slice: whole-history event/activity projection, markdown and lower-level surface formatting still need replacement.

This removes 3,252 Rust source lines. The source tree contains 559 Rust files and 173,148 lines, 4.8% below the pinned 181,882-line reference; these totals include inline unit tests. The integration-test tree contains 117 Rust files and 27,357 lines. The two new implementation files contain 423 and 48 lines. No backend or dependency changes were needed. The synthetic composite/block/navigation APIs had no production consumers outside this TUI. `AppState::run_transcript_pager` now returns the existing `PagerError` directly instead of the deleted integration error wrapper.

## Functional evidence

`source.json` records 562 candidate files: all 559 production-source-tree files, the runtime probe, public benchmark and shared journey fixture. The candidate is based on `5a62cb83`; the original production code remains pinned to `1bb0f98988670a5f4b48cdf749b455a79cfdaa82` in the isolated checkout. `binaries.json` identifies the release benchmark binaries, not shipped application executables.

The pager characterization passed before replacing the composite (`pager-before.log`). It uses real events and verifies exact exported text and terminal suspension/restoration. Public navigation now covers failed and streaming jumps, adjacent messages, resizing and output arriving while the terminal has zero rows. This replaces structural checks of the deleted engines; raw-data security and useful viewer/scroll tests remain.

Independent review found that the first replacement lost timeline status changes while the terminal height was zero. The new journey failed with selected turn 3 instead of 4 (`zero-red.log`), while the original passed (`navigation-reference.log`). Updating the existing outline with its last valid geometry fixes the failure (`zero-green.log`). These logs precede the journey's Unicode extension.

The extended journey compares 21 joined emoji with 21 ASCII pairs of equal display width. The original sums the emoji's scalar widths and reports different jump offsets at widths 40 and 80 (`unicode-reference.log`). The replacement measures string display width and passes (`unicode-candidate.log`). This is documented as R9. All 12 original/candidate post-jump buffers are nevertheless cell/style/text identical (`unicode/comparison.json`); this fixture establishes a numeric geometry correction, not a visible improvement. `unicode/journey.rs` and `unicode/reference-journey.rs` are the same fixture; the copies differ only by rustfmt whitespace and trailing commas.

All 539 frozen checkpoints pass with the same two R8 gap corrections and no new snapshot exceptions. The final serial run passes 1,737 tests and times out one viewer capture at the unchanged 20-second limit while release compilation runs (`checks.log`). That test passes in 16.8 seconds after compilation finishes (`timeout-rerun.log`). The timeout is retained, not counted as an initial pass. Seven explicitly gated PTY checks, scoped all-target/all-feature Clippy, workspace check, formatting and test-suite gates pass.

```sh
cargo nextest run -p harness-tui --all-features --profile ci -j1
cargo nextest run -p harness-tui --all-features --profile ci -j1 -E 'test(native_tool_viewer_capture_uses_the_enter_handler)'
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --all-features --profile ci --run-ignored all --test p0_01_pty_recorded --test p0_02_pty_recorded --test p0_03_pty_recorded -j1
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
```

## Browser captures

The 40×24 and 120×40 streaming PNGs are byte-identical to the original captures. Cells, text and cursor also match. These are controlled-timestamp ANSI replays through the same xterm.js, Chromium and font, not live PTY latency measurements.

The separate `selection` directory contains a real PTY/xterm run. Before and after selection, cells, text and cursor equal the previous approved candidate. It exits naturally with status zero, restores termios and terminal protocols, and removes the process group, socket, browser profile and temporary root. This debug-probe capture provides behavioral evidence only.

```sh
HARNESS_TUI_REFERENCE_FRAMES=/tmp/frames cargo nextest run -p harness-tui --all-features --profile ci --test rewrite_reference_test
# Copy the two stream-markdown ANSI files into INPUT_DIR, then:
node scripts/qa/render-recorded-frames.mjs INPUT_DIR .omo/evidence/outline-streaming
cargo build -p harness-tui --all-features --example rewrite_probe
node scripts/qa/capture-rewrite-selection.mjs target/debug/examples/rewrite_probe .omo/evidence/outline-selection
HARNESS_TUI_OUTLINE_FRAMES=/tmp/outline-frames cargo nextest run -p harness-tui --all-features --profile ci --test transcript_timeline_test
```

## Release measurements

Both builds use `--release --all-features`. All compilation, tests and browser captures finished before sampling. The checkouts run serially:

```sh
python3 scripts/measure-tui-rewrite.py --root REFERENCE --output /tmp/reference --allocations
python3 scripts/measure-tui-rewrite.py --output /tmp/candidate --allocations
```

Each workload has three runs of 200 measured frames plus a separate glibc `memusage` run. Non-startup workloads have 1,000 history turns. Timings include public input/event handling, preparation, paint, diff and ANSI encoding. Allocation totals include fixture construction. These are not end-to-end terminal latency measurements.

| Workload | p95 reference / candidate (µs) | p99 reference / candidate (µs) | CPU ms/frame reference / candidate | RSS KiB reference / candidate | Allocated bytes reference / candidate |
|---|---:|---:|---:|---:|---:|
| stream-1000 | 8,866 / 8,619 | 9,307 / 9,152 | 4.75 / 4.10 | 51,456 / 40,020 | 744,455,373 / 461,394,627 |
| resize-1000 | 9,772 / 5,573 | 10,107 / 5,873 | 5.60 / 2.05 | 65,968 / 45,920 | 670,824,951 / 388,294,281 |

Against this paired reference, resize CPU falls 63.4%, allocations 42.1%, RSS 30.4%, p95 43.0% and p99 41.9%. Streaming allocations fall 38.0%, but CPU falls only 13.7%, RSS 22.2%, p95 2.8% and p99 1.7%. Whole-rewrite resource acceptance remains unmet, and the earlier frozen timing limits remain unchanged.

The other paired p99 increases stay within their permitted allowance: idle is 335 versus 315 µs, typing 351 versus 320 µs, and scroll 407 versus 386 µs. Typing CPU rises from 0.30 to 0.35 ms/frame. Scroll cold preparation rises 1.3%; startup allocated bytes and peak heap rise 0.04% and 0.11%. `performance/comparison.json` reports all six workloads, including these regressions. No runtime-idle, browser-latency or live-provider improvement is claimed from this renderer workload.

All six timed byte counts and oldest retained screens match in every repetition. The scroll workload's extra post-timing screen retains the documented R8 blank gap; the other five final screens match directly. History and output are retained. Live providers and unsupported terminal environments remain unverified.

# Direct transcript entry construction

Turn rendering now reads borrowed semantic parts and scans tool groups once. This removes ten grammar modules, repeated content/specification clones, unused metadata, and the identity-conversion trait used only to adapt internal tests. Measured sections retain each surface's rows once; test-only flattened text is derived from those rows and their geometry.

This remains a migration slice. The old composite state engine, markdown formatters, and lower-level surface formatting remain. The change removes 2,033 Rust source lines, including nine structural tests. The source tree has 572 Rust files and 176,400 lines, a 3.0% reduction against the pinned 181,882-line reference. The 50% reduction requirement remains open. The source total includes inline unit tests. The integration-test tree is unchanged in this slice: 120 Rust files and 28,320 lines. No backend contract changes or new dependencies were needed.

## Functional evidence

`source.json` records the candidate source compiled for these checks and measurements, based on `3b16dab6`. The original implementation remains pinned to `1bb0f98988670a5f4b48cdf749b455a79cfdaa82` in the isolated reference checkout. Source and binary receipts identify the exact tested trees; these are not shipped-executable claims.

The first direct builder drew a visible rail beside streaming prose. The old renderer reserved that column with a blank glyph. The existing oracle failed on this regression (`red.log`, `red-cells.json.gz`); the correction retains both the blank glyph and column geometry. All 539 checkpoints then pass (`green.log`, `cells.json.gz`). The original golden file is unchanged: 537 frames compare directly, with the same two documented R8 gap corrections as the previous slice. No new visual exceptions were added.

The final serial suite passes 1,774 tests with six skips. Seven explicitly gated PTY checks pass. Scoped all-target/all-feature Clippy, workspace check, formatting and test-suite gates pass. Nine removed tests checked the deleted grammar, private enum/field forwarding, or the unused group policy; retained public and rendering checks cover those user-visible behaviors.

```sh
cargo nextest run -p harness-tui --all-features --profile ci -j1
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --all-features --profile ci --run-ignored all --test p0_01_pty_recorded --test p0_02_pty_recorded --test p0_03_pty_recorded -j1
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
```

## Browser captures

The 40×24 and 120×40 streaming PNGs are byte-identical to the original captures. Cells, text and cursor also match exactly. These use recorded ANSI at controlled timestamps through the same xterm.js, Chromium and font; they are static frame replays, not live PTY evidence.

The separate `selection` directory contains a real PTY/xterm.js run. Before and after selection, cells, text and cursor equal the previous approved candidate. It exits naturally with status zero, restores termios and terminal protocols, and removes the process group, socket, browser profile and temporary root. This debug-probe capture overlaps release compilation and supplies behavioral evidence only; no latency claim uses it.

```sh
HARNESS_TUI_REFERENCE_FRAMES=/tmp/frames cargo nextest run -p harness-tui --all-features --profile ci --test rewrite_reference_test
# Copy the two stream-markdown ANSI files into INPUT_DIR, then:
node scripts/qa/render-recorded-frames.mjs INPUT_DIR .omo/evidence/direct-streaming
cargo build -p harness-tui --all-features --example rewrite_probe
node scripts/qa/capture-rewrite-selection.mjs target/debug/examples/rewrite_probe .omo/evidence/direct-selection
```

## Release measurements

Both builds use `--release --all-features`. Complete compilation before sampling, and run the checkouts serially without concurrent builds or browser captures:

```sh
python3 scripts/measure-tui-rewrite.py --root REFERENCE --output /tmp/reference --allocations
python3 scripts/measure-tui-rewrite.py --output /tmp/candidate --allocations
```

Each workload has three runs of 200 measured frames plus a separate glibc `memusage` run. Non-startup workloads have 1,000 history turns. Timings include public input/event handling, preparation, paint, diff and ANSI encoding. Allocation totals include fixture construction. They are not end-to-end terminal latency measurements.

| Workload | p95 reference / candidate (µs) | p99 reference / candidate (µs) | CPU ms/frame reference / candidate | RSS KiB reference / candidate | Allocated bytes reference / candidate |
|---|---:|---:|---:|---:|---:|
| stream-1000 | 8,809 / 8,634 | 9,418 / 9,138 | 4.75 / 4.05 | 51,348 / 41,852 | 744,451,196 / 473,008,071 |
| resize-1000 | 9,807 / 9,798 | 10,172 / 10,242 | 5.65 / 5.8 | 66,052 / 48,772 | 670,830,022 / 663,721,704 |

Streaming allocations fall 36.5%, CPU falls 14.7%, and RSS falls 18.5%. Resize RSS falls 26.2%, but allocations fall only 1.1% and CPU rises 2.7%. Streaming/resize cold preparation remains 5.4%/6.3% slower. Typing CPU is 0.35 versus 0.30 ms/frame; its p99 is 339 versus 325 µs. These results fail whole-rewrite resource acceptance.

The other paired p99 values remain within their allowed increases. The earlier frozen timing limits still are not met, and have not been replaced with these later, slower reference runs. Raw samples and allocation logs are under `performance`; `comparison.json` reports all six workloads rather than only the improvements.

All six timed byte counts and oldest retained screens match for every repetition. As documented for R8, the scroll workload's extra post-timing screen retains a blank gap that the original snaps past; the other five final screens match. No timed workload discards history or reduces output. Live provider and unsupported terminal environments remain unverified.


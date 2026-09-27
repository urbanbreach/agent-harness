# Terminal polling timeout correction

The level-triggered reader introduced during the event-loop replacement burned
CPU after each blocking wait. `filedescriptor` 0.8.3 truncated a positive
fractional millisecond to zero; Crossterm retried until the deadline elapsed.
The vendor patch rounds positive integer-millisecond waits up and saturates at
`c_int::MAX`. Zero and infinite waits retain their meanings. This corrects a
candidate regression, not a defect in the original reference's Mio reader.

The existing reader, parser, event ordering, bounded queue, typed errors and
joined shutdown remain unchanged. It still performs blocking 50 ms checks for
shutdown. The polling-free terminal-adapter goal remains open.

## Source and cost

The reference is `1bb0f98988670a5f4b48cdf749b455a79cfdaa82`; the preceding
candidate is `90739ad29213be5289341f049b2ab6bc9c64c352`. Reference production
source and its original executable remain unchanged. Identical current
measurement fixtures were added under distinct example names in that checkout.
`fixtures.json` records their hashes; `runtime/metadata.json` records binaries,
driver and vendor hashes. `runtime/source.patch` contains tracked changes at
measurement time. The new vendor files are recorded separately in that metadata
and in the committed source, not in that patch.

`vendor/filedescriptor/UPSTREAM.json` records the published archive checksum,
revision and original file hashes. `upstream.patch` is the complete source delta.
The copy adds 1,563 upstream Rust lines plus 29 net local lines, including a
22-line test block. Its original platform files retain their layout and exceed
500 lines. This is additional third-party maintenance, not TUI source reduction.
Two explicit inferred lifetimes remove current compiler warnings. The root
Cargo patch also applies to other users of this already-locked dependency.

TUI production source is unchanged. Existing PTY fixtures gain 16 lines. The
whole TUI source remains 167,968 lines, 7.65% below the original. The 50% source
reduction requirement and replacement of the remaining state/formatting code
are unfinished.

## Runtime measurements

`runtime/samples.json` contains 45 serial runs, three repeats for each build and
scenario. The unchanged driver uses a real 160x48 PTY, 1.5 s warmup and 4 s
measurement, with no cadence override. The middle repeat reverses build order.
Compilation and other captures finished before measurement. Values below are
medians; CPU is percent of one core and RSS is post-workload KiB.

| Workload | Original CPU | Before CPU | Patched CPU | Original RSS | Before RSS | Patched RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Idle | 0 | 1.50 | 0 | 13,244 | 10,104 | 10,092 |
| Startup | 0.75 | 1.75 | 0.50 | 12,968 | 9,880 | 9,992 |
| Typing | 11.75 | 11.75 | 13.00 | 14,408 | 11,716 | 11,748 |
| Burst | 11.25 | 12.25 | 10.75 | 12,708 | 9,744 | 9,700 |
| Slow output burst | 6.00 | 6.25 | 4.75 | 12,804 | 10,172 | 10,124 |

All three patched idle runs record zero CPU ticks, bytes and frames. Typing CPU
increases in this sample. The unchanged wall-clock driver injects unequal input
counts, with medians 2,037/2,251/2,321, so these are not fixed-input efficiency
comparisons. The cause of the increase is not established. Fresh typing and
burst measurements do not meet the whole-rewrite 30% CPU reduction requirement.
The frozen criteria are unchanged. Startup emits 26/23/23 frames in this short
window; this does not establish original-to-candidate animation cadence parity.

The separate poll interposer records 99,094 calls before the correction,
including 99,024 zero waits, versus 70 calls afterward, including one initial
zero wait and 69 blocking 50 ms waits. Both traces span about 3.5 s including
warmup. Logging affects execution, so these traces diagnose the cause only.
Use the uninstrumented runs above for CPU conclusions. `diagnostic-before/`
also retains the initial 30-run discovery sample. Its older original fixture
lacks an unused handoff-failure branch; final comparisons use identical fixtures.

These CPU measurements stop disposable children with SIGTERM and have no
terminal emulator. They do not prove restoration or browser-visible latency.

## Browser latency and restoration

The archive contains three serial reference/candidate pairs with 120 samples
per input, stream and resize scenario. Both use xterm.js 6.0.0, the same embedded
Adwaita Mono font, Chromium 152.0.7977.82, fixture and reduced motion setting.
Browser timing ends after the expected DOM marker and two animation callbacks;
raw PTY timing ends at the first complete synchronized frame. These include
different observation costs and must not be called renderer timings.

| Workload | Original browser p95/p99 ms | Patched browser p95/p99 ms | Original PTY p95/p99 ms | Patched PTY p95/p99 ms |
| --- | ---: | ---: | ---: | ---: |
| Input | 70.38 / 73.18 | 69.41 / 69.82 | 1.43 / 4.63 | 4.09 / 4.80 |
| Stream | 66.97 / 67.16 | 66.94 / 67.18 | 1.19 / 1.26 | 1.19 / 1.30 |
| Resize | 65.62 / 72.96 | 70.60 / 75.76 | 21.79 / 24.35 | 22.67 / 26.48 |

All six browser limits pass against both the frozen and fresh references with
the original +16.7 ms allowance. All nine screenshots are byte-identical across
the six runs. Final terminal cells/styles, text, cursor, modes and dimensions
match. `browser/comparisons.json` records the checked fields and hashes.

The separate workflow delivers a live notice through a 1,000-click burst in
65.57 ms, within its 1,000 ms regression bound. Rewind failure preserves the
draft, success restores the prompt and stale generations are ignored. All
seven browser runs exit naturally, restore termios and terminal protocols, and
close the process group, stdin, sockets, browser and temporary profile.

Normal exit, telemetry initialization failure and successor handoff failure
also pass the separate restoration probe. `/dev/full` restores termios; protocol
escape output cannot reach that terminal and is not claimed as verified.

## Checks and reproduction

The conversion table first failed on a 1 ns wait, then passed after correction.
The existing P0-04 PTY journey now submits one ordered 1,800-byte Unicode burst
plus its submit key in one write. Its acknowledgement requires exact full text.
All 1,666 deterministic TUI tests pass with seven configured skips. Seven gated
P0-04/P1-04 checks pass; the final P0-04 rerun passes after a test-only formatting
correction. The six P1-04 sessions cover Unicode/ASCII, three sizes, following,
detached viewports, resize bursts and reduced motion. Their captures and cleanup
receipt are archived. Scoped Clippy, workspace check, formatting and suite gates
pass. Logs include the expected red test and the corrected test-Clippy failure.

From the candidate workspace, with `REFERENCE` set to the retained checkout:

```sh
cargo nextest run --manifest-path vendor/filedescriptor/Cargo.toml
cargo nextest run --profile ci -p harness-tui --all-features
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tui --all-features --ignore-default-filter -E 'binary(p0_04_pty_recorded) | binary(p1_04_pty_recorded)'
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
cargo build --release -p harness-tui --example resource_probe --example rewrite_probe
cp crates/harness-tui/examples/resource_probe.rs "$REFERENCE/crates/harness-tui/examples/poll_timeout_resource_probe.rs"
cp crates/harness-tui/examples/rewrite_probe.rs "$REFERENCE/crates/harness-tui/examples/poll_timeout_rewrite_probe.rs"
cargo build --manifest-path "$REFERENCE/Cargo.toml" --release --locked -p harness-tui --example poll_timeout_resource_probe --example poll_timeout_rewrite_probe
python3 scripts/measure-tui-runtime.py --binary target/release/examples/resource_probe --output OUTPUT/runtime.json
node scripts/qa/measure-rewrite-latency.mjs target/release/examples/rewrite_probe .omo/evidence/OUTPUT 120
node scripts/qa/measure-rewrite-latency.mjs target/release/examples/rewrite_probe .omo/evidence/WORKFLOW --workflow-only
python3 scripts/check-tui-restoration.py --binary target/release/examples/resource_probe --output OUTPUT/restoration
```

Finish builds first, then measure serially. Repeat the measurement commands
with the reference binaries and three repeats; the exact local ordering and
paths are retained in `runtime/measure.py` and `browser/measure.py`. The original
restoration probe needs its previously documented reference-defect exemption.
Build the optional diagnostic shim with `cc -shared -fPIC -O2 poll-trace.c -ldl
-o /tmp/tui_poll_trace.so`; the archived trace drivers set its environment only
for their subprocesses. Do not use the shim for acceptance timings.

Linux is the verified platform. The Windows caller shares the conversion but
was not compiled or run here; macOS's separate select path is unchanged and
unverified. The provider updates are synthetic. This slice does not replace
the remaining state engine, prove long-session growth bounds, or provide final
whole-goal signoff. The preceding renderer limits were not rerun for this
dependency-only correction.

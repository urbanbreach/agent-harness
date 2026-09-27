# Interruptible Unix terminal ingress

The candidate reader at `584c7251d86559a81e659b1b0df47e8e1b95984c`
checked shutdown every 50 ms. Its selected Crossterm TTY source could also block
on a second read after receiving incomplete input. The red PTY run records
about 20 voluntary reader context switches per second, shutdown timeouts after
partial UTF-8 and unterminated paste, and a terminal-hangup timeout. These are
observations of the preceding candidate, not the original Mio-based reference.
The original reference checkout and executables remain retained.

The Unix reader now waits indefinitely on Crossterm's existing input, resize
and wake descriptors. Its owner sends stop, wakes the source and joins the
thread before terminal restoration. Drop uses the same path. A single read per
readiness notification keeps partial input interruptible. Buffered events drain
before another wait; readable bytes survive simultaneous hangup; EOF and
invalid/error readiness return a typed failure. Waking preserves nonmatching
protocol replies. No terminal input is injected and no parser or worker is
added. The bounded FIFO, normalization and public custom-source seam remain.

## Dependency cost and scope

The upstream parser queue and wake pipe are private. A finite outer drain was
rejected because a deschedule can exhaust that timeout before the private queue
is inspected, stranding input during an indefinite outer wait. The local
Crossterm patch exposes its existing blocking poll and fallible waker. It never
constructs `EventStream`, whose worker would not be owned by this runtime.

`vendor/crossterm/UPSTREAM.json` records 62 original file hashes, the published
0.29.0 archive checksum and revision. `crossterm.patch` is the complete delta:
three modified upstream files and one 31-line adapter. The copy contains 12,332
upstream Rust lines and 66 net local lines. Original third-party file layouts,
including files above 500 lines, are retained. This is added maintenance, not
TUI source reduction. The dependency's existing unused-parentheses compiler
warning is retained. The prior `filedescriptor` finite-timeout patch remains.

Owned TUI source grows by 49 lines to 167,131, only 8.11% below the original
181,882. The reader file is 200 lines. No backend source or authority changes.
The root Cargo patch also applies to CLI users of Crossterm; their parsing and
raw-mode implementation are unchanged. The `event-stream` feature is enabled
only on Unix. Other platforms retain the timed reader path. Linux is verified;
macOS, other Unix systems and Windows are not verified by these captures.

## PTY behavior and resources

`baseline.json` predates production changes and freezes the limits: at most two
idle voluntary switches per thread per second, no new idle output, shutdown
within one second and hangup exit within two seconds. The existing browser
allowance remains +16.7 ms for input, stream and resize p95/p99.

The final green run records zero idle switches and output in all five cases.
Idle, partial UTF-8, unterminated paste and resize-only shutdown complete in
10.10–10.34 ms and restore exact termios plus enabled protocols. The driver's
10 ms observation loop limits timing precision. With SIGHUP deliberately ignored,
hangup returns exit code 1 in 10.10 ms and stderr identifies a terminal-event
poll failure. Termios cannot be inspected after the PTY master closes.

Four successive runtime entries preserve then restore terminal state. Normal
exit, telemetry setup failure, successor setup failure and `/dev/full` restore
termios. Protocol output cannot reach a terminal through `/dev/full` and is not
claimed as verified. The existing bounded-queue check now also exercises owner
Drop and checks source destruction before return. The gated PTY journey still
submits and acknowledges the full ordered 1,800-byte Unicode burst.

Six serial release idle runs use the same 160x48 PTY workload, 1.5 s warmup and
8 s sample. Both builds record zero CPU ticks, frames and bytes in every run;
there is no measurable CPU reduction from this already-zero baseline. Median
RSS is 10,232 KiB before and 10,088 KiB afterward, with no within-run growth.
The demonstrated improvement is removal of recurring reader wakeups. These
short samples do not establish long-duration memory or sustained-workload gains.

## Browser comparison

Three serial release pairs use 120 samples per input, stream and resize
workload, with the middle pair's build order reversed. The same xterm.js,
Chromium, Adwaita Mono font, terminal geometry and offline fixture are used.
These timings include DOM observation and two animation callbacks; raw PTY
measurements end at the first complete synchronized frame. Neither measures
isolated renderer cost. Median run percentiles are:

| Workload | Before browser p95/p99 ms | Candidate browser p95/p99 ms | Before PTY p95/p99 ms | Candidate PTY p95/p99 ms |
| --- | ---: | ---: | ---: | ---: |
| Input | 69.05 / 69.62 | 69.08 / 69.57 | 4.07 / 4.79 | 4.25 / 4.80 |
| Stream | 67.03 / 67.16 | 67.06 / 67.21 | 1.17 / 1.25 | 1.17 / 1.24 |
| Resize | 70.85 / 72.73 | 68.91 / 71.31 | 22.79 / 28.13 | 22.21 / 23.50 |

All six browser limits pass against both the fresh predecessor and the frozen
original reference. Idle terminal output is zero. A separate 1,000-click burst
still permits a live update in 48.81 ms, below its existing 1,000 ms limit.
Rewind failure preserves the draft, success restores the prompt and stale
responses are ignored. All seven browser runs exit naturally, restore terminal
state and close their process group, sockets, browser and temporary profile.

Eight of nine screenshots are byte-identical across all six paired runs. The
resized screenshot matches in five runs; the second predecessor run displays
53 seconds while the others display 54. Comparison identifies exactly two
changed digit cells at zero-based row 32, columns 19 and 116, with identical
styles. All 176 changed pixels lie inside those two cells. Text, cells, lines
and scrollback contain this same elapsed-clock difference; all remaining
terminal fields match. The elapsed label is driven by real time in this latency
fixture. These captures are therefore not a controlled-time animation test.
Raw captures are retained, and the comparator records the difference explicitly
rather than replacing a screenshot. Parser callback counts vary with output
chunk grouping and are recorded separately.

`browser-captures.tar.gz` contains all seven raw captures. The comparator and
`browser-comparisons.json` preserve exact hashes, changed cells, pixel bounds
and cleanup observations. Existing controlled-time animation and reference
journey tests pass within the TUI suite; their implementation is unchanged.

## Verification and known failures

The workspace run executes 1,949 tests: 1,929 pass, 20 fail and eight are skipped.
All TUI tests pass. Every candidate failure also fails on the pre-change source
export under the same environment. That export additionally fails two unchanged
artifact-root guards because its checkout is under `/tmp`; the full raw results
are retained. The managed worktree tools did not return, so this comparison uses
a `git archive` export, not a replacement of either retained reference checkout.

The CLI tests discover a local global configuration with a missing provider type.
No user configuration was edited or copied into evidence. With a fresh empty
`XDG_CONFIG_HOME` and unchanged `HOME`, both versions run 162 CLI tests: 157 pass,
five fail and one is skipped. The identical remaining failures are four stale
model assertions (two tests registered twice) and an old terminal-error text
assertion. The full workspace suite remains red; these are not new reader
regressions and are not presented as successful verification.

Seven gated P0-04/P1-04 checks, workspace all-target/all-feature Clippy, formatting
and test-suite gates pass. The new behavioral check failed before implementation.
An initial candidate run passed reader checks but exposed a driver defect: it
counted teardown's synchronized-update marker as a fifth session. Quit bursts
are now capped at the expected session count; that excluded initial run is
retained. Review also caught partial-setup SIGWINCH ownership and hangup evidence
that could have accepted a signal exit; both were corrected before the final run.

Independent review approved this bounded change after checking source hashes,
raw samples, the clock-label exception, ownership checks and baseline failures.
`review.json` records that approval; it is not whole-rewrite signoff.

## Reproduction and limits

`checks.json`, `baseline-tests.json` and `cli-isolated.json` record commands and
results. `binaries.json` identifies retained executables. `before-fixture.patch`
adds only the controller-driven stop operation to the predecessor probe; the
browser fixture is identical for both binaries. `fixtures.json` also records
the additional candidate-only handoff branch in the resource probe. The measured
idle branch and its inputs are unchanged. `source-files.json`,
`source.patch` and the upstream receipt identify the measured source. The
resource and browser driver is `measure.py`; it finishes compilation before
running serially and reverses build order for the middle repetition.

```bash
cargo build --release -p harness-tui --all-features --example resource_probe --example rewrite_probe
python3 scripts/check-tui-restoration.py --binary target/release/examples/resource_probe \
  --live-binary target/release/examples/rewrite_probe --output /tmp/tui-restoration
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tui --all-features \
  --ignore-default-filter -E 'binary(p0_04_pty_recorded) | binary(p1_04_pty_recorded)'
```

The broader rewrite remains incomplete: state/text implementation replacement,
50% source reduction, sustained typing/burst CPU targets, the original startup
cadence discrepancy, long-duration resource evidence, full feature/removal
review and live-provider/other-platform signoff remain open. Removing the Unix
reader timer does not close those requirements.

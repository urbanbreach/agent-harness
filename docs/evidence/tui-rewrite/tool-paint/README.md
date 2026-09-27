# Tool painter replacement

Candidate source is `a7d6d8e17168ea339b9d660456b308f3839d3ff1` plus
`source/source.patch.gz`. The original remains pinned at
`1bb0f98988670a5f4b48cdf749b455a79cfdaa82`. This replaces tool dispatch and detail
painting; it does not complete the TUI rewrite. No backend contract or intended
UI behavior changes. Nothing was pushed.

The renderer now borrows detail blocks and carries only theme, width, background
and output rows. Ordinary inline/block headers share one path. Task rows retain
bounded child-session targets; shell rows retain command/description handling and
hint-only click targets. Nested file details use their original generic/full-output
context without cloning synthetic tool sections. Hooks, completion rails and hover
styling still run after details, in that order.

The unreachable nested-card helpers, transcript Markdown/todo variants and ignored
bash hint field are removed. The active todo pane is unchanged. Header/error formatting,
read gutter/tab rules, shell title wrapping and completion rails remain stable leaf
rules. The existing themed-surface check now exercises a reachable edit detail and
asserts both tool surfaces exist.

The change removes 577 lines. Every replacement source file is at most 391 lines.
The TUI source tree contains 167,082 lines in 574 files, 8.14% below the original.
The 50% source-reduction target remains unmet.

## Verification

All 1,668 deterministic TUI tests pass with seven configured skips. Scoped
all-target/all-feature Clippy, workspace check, formatting and suite gates pass.
A pre-change mutation removed the patch-file click target; the existing disclosure
test failed. The final source passes that test. The three header checks move with
unchanged assertions. No additional test function is added.

All 555 frozen reference records and ANSI frames match. All 733 controlled animation
frames match text and ANSI. Fresh hashes link to the preceding published archives.
Additional pre-change captures match 445 tool-body frames, 234 ordering frames and
93 interaction frames, including their recorded interaction state. Both sides are
preserved under `frames`; producer metadata is excluded from byte comparisons.
The fixture files are unchanged from the preceding commit. Browser manifests describe
the later ANSI-replay checkout; the archived pre-change ANSI was produced before edits
at the baseline commit recorded under `source`.

Six paired xterm.js/Chromium tool-detail captures match PNG pixels, cells and modes.
Browser render callback differences are recorded explicitly.
Paired actual-runtime PTY/xterm selection runs also match pixels/cells/modes and all
17,850 raw terminal bytes. The candidate records one extra parse callback and render
callback in both frames; these callbacks can group the identical bytes differently. Both
children exit normally, restore termios and protocol modes, and release the temporary
workspace, process group, sockets, browser context/profile and ports. Seven gated
P0-03/P1-04 PTY checks pass; all six P1-04 children exit and PTYs close.

These are deterministic Linux checks. There is no new other-OS or live-provider
verification. Recorded-frame comparisons do not establish real animation cadence.

## Release measurements

The unchanged tool fixture alternates disclosure for 200 completed turns with read,
shell, grep and generic tools. Ten warmups precede 200 measured frames. The boundary
includes the disclosure seam, preparation, painting, Ratatui diffing and Crossterm
counting-sink encoding. It excludes keyboard dispatch and terminal-emulator paint.

All six existing tool limits and fourteen general-workload limits pass unchanged.
Three timing runs and a separate `memusage` run per executable are retained for each
workload. Builds and browser captures finished before the serial measurements.

| Measurement | Original | Preceding | Candidate |
| --- | ---: | ---: | ---: |
| p95, µs | 12,013 | 9,008 | 8,871 |
| p99, µs | 12,099 | 9,056 | 8,939 |
| CPU, ms/frame | 9.95 | 7.05 | 6.9 |
| Allocated bytes, whole process | 9,251,013,983 | 7,928,203,764 | 7,927,932,588 |
| Peak heap, bytes | 30,279,848 | 25,377,327 | 25,379,438 |
| RSS, KiB | 45,548 | 39,188 | 39,108 |

Performance stays close to the preceding build; this change primarily removes code.
All twelve tool runs preserve output, event/frame counts and encoded bytes.
Seventy-two further runs cover startup, forced idle redraw, typing, streaming,
scrolling and resize. All 24 preceding/candidate output pairs match. Original-relative
scroll differences reproduce the preceding published screens and accepted R8 correction.
General synthetic typing CPU rises from 0.10 to 0.15 ms/frame with coarse process-clock
ticks, and startup cold rendering rises from 622 to 661 µs. Neither metric has a gate
in this 20-check set; the existing gates still pass. These synthetic renderer workloads
do not establish runtime idle or sustained-input performance. Allocation totals include construction, warmups, checks and teardown.

The initial replacement measured bounds for shell rows without a click target,
raising allocations from 7,928,216,140 to 8,217,282,020 bytes. The shared bounded-hit
helper now returns immediately when there is no target. Initial source, checks,
measurements and browser evidence remain in `initial/validation.tar.gz` and are
excluded from final acceptance. The final samples above all use the corrected source.

## Reproduction and remaining work

`checks/checks.py`, `checks/finish.py`, `frames/compare.py` and the scripts under
`browser`, `performance` and `general` contain the commands. Raw archives include
binary hashes, run order, samples, cell snapshots, screenshots and cleanup receipts.
The benchmark scripts use retained `/tmp` paths. Rebuild the original and preceding
commits with the identical published fixtures; apply the source patch for the candidate.
Obtain each performance executable with:

```sh
cargo nextest list --release -p harness-tui --all-features \
  --test rewrite_performance_test --list-type binaries-only --message-format json
```

The fixture and journey helper are included under `source`; common nextest Cargo
metadata is compressed under `performance`. The pre-change baseline receipt records
the reused acceptance limits before production edits.

Remaining state/text formatter replacement, terminal-reader polling, sustained runtime
CPU, the startup cadence discrepancy, long-duration resource evidence and final feature
and removal review remain open. `review.json` is independent approval for this change,
not signoff for the complete rewrite.

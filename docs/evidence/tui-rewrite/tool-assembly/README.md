# Tool-row assembly replacement

Candidate source is `fce5873140fa0cf4e9cf9448194113f4505ce541` plus
`source/source.patch.gz`. The original remains pinned at
`1bb0f98988670a5f4b48cdf749b455a79cfdaa82` in the isolated reference checkout.
No backend contracts changed. This replaces the tool-row assembly path, not the
remaining TUI state engine or painter. Nothing was pushed.

The assembler now constructs one row, dispatches tool-specific content into it,
and applies shared detail and subtitle rules. The old nested six-value tuple
builder, repeated style/visibility returns, duplicate generic-output construction,
six uncalled subagent helpers and unreachable todo row branches are removed.
Diff and error helpers retain their established behavior in focused files. All
replacement source files are below 500 lines; the largest is 425 lines.

R17 records a removed side effect. The former assembler called the todo parser
for every tool, including its filesystem artifact fallback, then discarded that
result for every reachable tool family. Transcript todo calls are already hidden
by the production entry point. Removing this eager call removes those unnecessary
artifact reads. The active todo pane's parser remains. This finding comes from
tracing the source and callers; it is not a syscall-count measurement. No intended
visible behavior or reference snapshot changed.

## Verification

- All 1,668 deterministic tests pass, with seven configured skips. Clippy,
  workspace check, formatting and suite gates pass on the final source.
- The five existing assembler checks moved with formatting changes only. Existing
  behavioral coverage exercises native/MCP identity, question answers, disclosure,
  command errors, diffs, coalescing, background tasks and terminal sanitization.
- All 555 reference records and their ANSI output match the preceding published
  captures. All 733 controlled animation frame pairs also match byte for byte.
- Six xterm.js 6.0.0/Chromium captures match pixels, cells and terminal modes.
  Parsing counts are equal. Five captures have one fewer browser render callback;
  these observer callbacks do not measure application redraw cadence.
- Seven gated P0-03/P1-04 PTY checks pass. The P1-04 owner reports all six children
  exited and all PTYs closed.
- The real PTY/xterm rewind and 1,000-click workflow passes. Its single visible
  response observation is 48.56 ms, not a percentile or latency improvement claim.
  The child exited naturally. Termios, terminal modes, sockets, browser contexts,
  profile and ports were restored or closed.
- Normal exit, telemetry failure and failed handoff restore termios and checked
  terminal modes. `/dev/full` verifies termios only, since terminal escapes cannot
  reach the emulator when output fails. This is synthetic Linux evidence.

`checks/run.py`, `checks/*.json` and `checks/logs.tar.gz` record commands and results.
The final full run followed both unused-import cleanups. Earlier post-review
checks are also retained. Frame and browser archives contain the raw comparisons;
`runtime/workflow.json.gz` contains the complete real-terminal report.

## Performance

The new `tools` case extends the existing release fixture. It loads 200 completed
turns containing read, shell, grep and generic tools, then toggles global output
disclosure through the explicit test seam. Untimed assertions check retained tool
IDs and that output appears and disappears. The timed boundary includes disclosure
projection, preparation, painting, Ratatui diffing and Crossterm counting-sink
encoding. It excludes keyboard dispatch and terminal-emulator paint.

Six numeric limits were frozen from the preceding build before production edits.
All six pass. There are twelve serial runs across original, preceding and candidate
executables: three timing repeats and one separate allocation run per build, each
with ten warmups and 200 measured frames. Builds and browser captures had finished
before measurements. All output, history, frame-count and terminal-byte receipts
match across the three builds. Baseline measurements preceded implementation;
original and candidate measurements followed validation. This is a regression
check, not a statistically established speedup.

| Measurement | Original | Preceding | Candidate |
| --- | ---: | ---: | ---: |
| p50, µs | 8,526 | 6,567 | 6,725 |
| p95, µs | 11,856 | 15,043 | 15,359 |
| p99, µs | 12,046 | 15,157 | 15,445 |
| CPU, ms/frame | 9.90 | 10.50 | 10.65 |
| Allocated bytes, whole process | 9,251,009,010 | 7,986,409,532 | 7,956,018,736 |
| Peak heap, bytes | 30,279,848 | 25,621,538 | 25,619,429 |
| RSS, KiB | 45,500 | 39,480 | 39,464 |

Allocations fall 0.38% from the preceding build; memory is essentially unchanged.
The candidate's tool-disclosure p99 remains 28.2% above the original, and CPU is
7.6% higher. The new workload exposes a remaining resource/responsiveness gap in
the broader rewrite. Passing limits against the preceding build does not resolve
it or establish end-to-end latency parity. Earlier general-workload evidence is
recorded with its own commits; these six checks cover this tool workload only.

`performance/raw.tar.gz` contains every raw sample, run order, binary hash and
nextest execution manifest. `measure.py` and `compare.py` reproduce the runs and
comparisons. Rebuild retained binaries if their `/tmp` paths are absent. Use the
recorded commit plus source patch for the candidate, and the same published
`rewrite_performance_test.rs` and `rewrite_journey.rs` fixtures on both the original
and preceding commits. Obtain nextest manifests with `cargo nextest list --release
-p harness-tui --all-features --test rewrite_performance_test --list-type binaries-only
--message-format json`. `acceptance-before-implementation.json` and the source edit
start receipt preserve the acceptance chronology. Allocation totals include setup,
warmups, checks and teardown; CPU and latency samples cover the measured frames.

## Remaining work

Production assembler code falls from 1,435 to 1,063 lines, counting the new test
module registration as production. Including moved tests, it falls from 1,641 to
1,263 lines. One unused transcript import is also removed, while the performance
fixture grows by 48 lines. Whole TUI source is 167,627 lines, 7.84% below the
original. The source-reduction target remains unmet.

The tool-heavy tail-latency gap, retained title/paint helpers, older state engine,
polling, sustained runtime CPU target, startup cadence discrepancy, long-duration
memory evidence and final feature/removal review remain open. Live providers and
other operating systems are unverified. This slice does not complete the rewrite.

`files.json` hashes the artifacts. The independent source review approved this
implementation for validation; the final evidence decision is recorded separately.

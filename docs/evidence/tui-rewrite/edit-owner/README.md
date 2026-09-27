# Edit presentation ownership

Candidate source is `27df3ab7` plus `source/source.patch.gz`. The runnable
original remains pinned at `1bb0f98988670a5f4b48cdf749b455a79cfdaa82` in the
isolated reference checkout. This is one state-engine replacement, not completion
of the TUI rewrite. No backend contracts changed and nothing was pushed.

The shared edit reducer replaces the live and settled Proposed/Applied/Rejected
match blocks. Complete histories now derive edits from active events instead of
copying stale presentation state over that result. Live sequence assignment and
settled maximum sequence handling remain distinct. Other tool enrichment remains.

R16 is a deliberate correction. An earlier tool's proposal survives while a later
turn applies or rejects it. Rewinding that later user turn previously retained the
Patch title or rejection text. The public test uses selectable user rewind points,
checks both results, and reopens the history. The original fails on the Patch title;
the candidate passes. Existing reference snapshots were not changed.

Capped inline histories keep the original prior-edit carry after the event fold.
Those slices can lack the proposal, so rebuilding from their remaining events
would lose its summary and digest. The focused check first failed without carry,
then passed with it, including a rewind of a later prompt. It also passes on the
original. Exact removal of discarded edit state in capped inline views remains an
existing limitation; this change does not invent missing provenance.

## Verification

- 1,668 deterministic tests pass, with seven configured skips. Clippy, workspace
  check, formatting and suite gates pass. The extended capped-slice check also
  passed separately after its final rewind assertion was added.
- 555 reference records match: 543 main, eight plan and four wrapping records.
  All 733 controlled animation frames also match the preceding build byte for byte.
- Six xterm.js 6.0.0/Chromium screenshots have identical pixels, cells and modes.
  Parsing counts match. The three thinking captures have one additional browser
  render callback; the three running-tool captures have equal callback counts.
  These observer callbacks do not measure application redraw cadence. The browser
  baseline is the retained `d7df6d0f` capture in `settlement-suffix`; the preceding
  `27df3ab7` candidate also matched those six images in that published comparison.
- Seven gated P0-03/P1-04 PTY tests pass. All six P1-04 children exited and PTYs closed.
- The real PTY/xterm rewind journey and 1,000-click burst pass. Its single live
  response observation is 66.20 ms, not a latency percentile or acceptance claim.
  The child exited naturally; termios, protocol modes, sockets and browser resources
  were restored or closed.
- Normal exit, telemetry failure and failed handoff restore termios and checked
  terminal modes. `/dev/full` verifies only termios because output cannot reach the
  terminal. Synthetic Linux evidence does not verify live providers or other OSes.

`checks/logs.tar.gz` includes candidate red/green checks, the pinned-original
rewind failure and trimmed-slice success, and the final verification logs.
`checks/*.json` and `checks/run.py` record commands and environment settings.
The original's test-only adapter changes `events()` to `events.iter()` for its older
inspection API. It does not alter production code. Source and reference test
receipts are in `source/`.

## Performance

All nineteen existing limits pass without changes. There are 84 serial release
runs: three timing repeats for seven workloads on original, preceding and candidate
binaries, plus separate allocation runs for every pair. No builds or browser
captures ran during these measurements. Each measured workload uses 200 frames
following ten warmups, with identical fixtures, geometry and content checks.

| Measurement | Preceding build | Candidate |
| --- | ---: | ---: |
| Streaming p99, µs | 981 | 989 |
| Resize p99, µs | 2,729 | 2,740 |
| Settlement p99, µs | 1,382 | 1,373 |
| Settlement CPU, ms/frame | 1.20 | 1.20 |
| Settlement allocated bytes | 347,796,347 | 347,788,617 |
| Settlement RSS, KiB | 47,708 | 47,584 |

Allocations and memory are essentially unchanged. The general renderer/encoder
workloads do not establish edit-heavy throughput or end-to-end speed. The large
settlement improvement over the original belongs to the preceding implementation.
All 28 preceding/candidate output/content/count pairs match; original settlement
output also matches. `performance/summary.json` has every metric.

Raw samples, nextest binary metadata and SHA-256 receipts are in
`performance/raw.tar.gz`. `measure.py` and `compare.py` reproduce the serial order
and comparisons. They use the retained `/tmp` binary paths recorded in the archive;
rebuild or restore those paths from the recorded commits/patch and the unchanged
`rewrite_performance_test` fixture before rerunning. The original fourteen limits
are retained in the comparison files; the five settlement limits retain their
pre-implementation acceptance record. Timings include public ingestion, preparation,
painting, Ratatui diffing and Crossterm counting-sink encoding as appropriate to
the scenario. They exclude terminal-emulator paint.

## Remaining work

This slice removes 45 production lines and adds 117 unit-test plus 129 integration-
test lines. Whole TUI source grows by 72 lines to 168,006, a 7.63% reduction from
the original. Both removed edit transition blocks and the unconditional prior-edit
copy are absent; other presentation merging and most legacy state code remain.

The 50% source target, full state/formatter replacement, polling removal, sustained
runtime CPU target, startup cadence parity, long-duration memory evidence and final
feature/rewrite review are still unfinished. This evidence cannot close those goals.

`files.json` hashes the published artifacts. Archives preserve the raw frames,
cell records, PTY captures and restoration output. The source reviewer approved
this bounded change; `review.json` records the final evidence review.

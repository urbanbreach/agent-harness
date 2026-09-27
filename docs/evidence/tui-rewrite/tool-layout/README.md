# Tool-disclosure selection cost

Candidate source is `125143b3a603223894c0f9c60a84dda03e9ae8e7` plus
`source/source.patch.gz`. The original remains pinned at
`1bb0f98988670a5f4b48cdf749b455a79cfdaa82`. This change reduces selection-row
construction cost; it does not complete the renderer or state-engine replacement.
No backend contract or intended UI behavior changes. Nothing was pushed.

Temporary release instrumentation attributed about 6.9 ms of each disclosed frame
to selection-row construction across the two scrollbar widths. The old builder
visited and appended every printable ASCII character separately, including tool
panels' full-width padding. It now copies printable ASCII spans in row-sized slices.
Non-ASCII and control spans retain the grapheme path. Rail replacement, wrapping,
leading/trailing bounds and split-span combining marks retain their behavior.

Prepared selection rows remain owned by the layout. Removing them and reconstructing
an entire requested surface would revive the previously measured long-selection
regression. No lazy conversion, cache, new index or dependency is added. The shared
row builder grows by 32 lines to 193. The whole TUI source tree contains 167,659
lines in 569 files, 7.82% below the original. The 50% reduction target remains unmet.

## Correctness and terminal evidence

- All 1,668 deterministic TUI tests pass, with seven configured skips. Scoped
  all-target/all-feature Clippy, workspace check, formatting and suite gates pass.
- A temporary comparison against the preceding row builder matches 41,120
  projections, including every character-boundary span split in its corpus,
  widths 0–16/40/155/156, rails, alignment, controls, spaces, combining marks and
  wide graphemes. The final-source run also passes 70 existing selection, link and
  clipboard checks. The oracle and runner are preserved under `diagnostic`; their
  temporary test registration is removed. No permanent test source changes.
- All 555 reference records and ANSI frames match the preceding published matrix.
  All 733 controlled animation records also match their text and ANSI. The fresh
  hashes in `frames/comparison.json` point to the identical existing raw archives;
  those archives are not duplicated here.
- Six xterm.js/Chromium recorded frames match pixels, cells and terminal modes.
  Four have one more or fewer browser render callback; parsed counts are equal.
  Observer callbacks do not establish application animation cadence.
- Paired real PTY/xterm selection runs match both the unselected and highlighted
  screenshots, cells and terminal modes. Only browser render counts differ. Both
  children exit normally, restore termios and protocol modes, and release PTYs,
  sockets, process groups, browser contexts/profiles and ports.
- Seven gated P0-03/P1-04 PTY tests pass. The P1-04 receipt confirms all six children
  exited and all PTYs closed. These are synthetic Linux checks; no new live-provider
  or other-OS verification is claimed.

The final oracle records the production row-file hash. `checks` contains commands
and logs; `browser/captures.tar.gz` contains raw terminal output, cell snapshots,
PNG files and cleanup reports. `pty` preserves the gated capture evidence.

## Release measurements

The unchanged public `tools` fixture alternates global disclosure for 200 completed
turns containing read, shell, grep and generic tools. Ten warmups precede 200 measured
frames. The boundary includes the disclosure seam, preparation, painting, Ratatui
diffing and Crossterm counting-sink encoding. It excludes keyboard dispatch and
terminal-emulator paint.

Limits were frozen before production edits. They retain the preceding resource
limits and require tool p99 to meet the original-relative 10%/100 µs rule and CPU
to be no worse than the recorded original. The preceding build failed both tightened
checks. All six pass on the final candidate without changing a limit.

| Measurement | Original | Preceding | Candidate |
| --- | ---: | ---: | ---: |
| p95, µs | 12,132 | 15,481 | 9,385 |
| p99, µs | 12,556 | 15,639 | 10,031 |
| CPU, ms/frame | 10.10 | 10.70 | 7.10 |
| Allocated bytes, whole process | 9,251,027,640 | 7,956,024,219 | 7,928,219,472 |
| Peak heap, bytes | 30,281,960 | 25,621,541 | 25,379,439 |
| RSS, KiB | 45,476 | 39,416 | 39,140 |

Against the preceding build, tool p99 falls 35.9% and CPU falls 33.6%. Allocations
fall 0.35% and RSS 0.70%; this is primarily a CPU improvement. The candidate is also
below the original's tool p99 and CPU. All twelve tool runs preserve visible output,
oldest history, event/frame counts and terminal bytes across the three executables.

Seventy-two additional serial runs cover startup, forced idle redraw, typing,
streaming, scrolling and resize. All fourteen existing timing/resource limits pass.
All 24 preceding/candidate output pairs match. The original scroll screen differs
as before: both sides exactly reproduce their previously published settlement-suffix
screens, including the accepted R8 viewport correction. Forced redraw and this
synthetic typing workload do not establish runtime idle or sustained input targets.

Each workload uses three timing repetitions and a separate `memusage` run per
executable. Timing summaries are medians; allocation totals include setup, warmups,
checks and teardown. Builds and browser captures finished before final measurements.
The initial nested-guard implementation passed tests but failed Clippy. Its validation
and measurements remain in `initial/validation.tar.gz`, excluded from final acceptance.
Temporary profiling is also excluded. Final measurements are not selected from those
initial samples.

## Reproduction and remaining work

`performance/measure.py` and `general/measure.py` record execution manifests, binary
hashes, run order and raw samples in their respective archives. Use the original and
preceding commits plus the identical published fixtures; use the source patch for the
candidate. Obtain each executable with:

```sh
cargo nextest list --release -p harness-tui --all-features \
  --test rewrite_performance_test --list-type binaries-only --message-format json
```

The measurement scripts use retained `/tmp` paths. Rebuild/copy those executables
and unpack their recorded metadata if the paths are absent. The main fixture and
journey helper are included under `source`; `performance/cargo-metadata.json.gz`
contains the common nextest cargo metadata. `diagnostic/check-rows.py` temporarily
registers the preserved oracle and restores the source in `finally`; unpack its
inputs at the recorded `/tmp/tui-tool-layout` paths before running it.

The older state engine, remaining tool/text painters, terminal-reader polling,
sustained runtime CPU target, startup cadence discrepancy, long-duration resource
evidence and final feature/removal review remain open. This change resolves the
measured tool-disclosure regression only. `review.json` records the independent
review decision for this scope; it is not whole-rewrite signoff.

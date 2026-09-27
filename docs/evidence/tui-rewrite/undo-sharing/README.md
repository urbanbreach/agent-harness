# Shared undo snapshots

The predecessor is `ae0fdaab99e2d1c88f9e141253075a76065f6b9f`.
Each edit previously owned two full snapshots, duplicating the preceding edit's
result as the next edit's starting point. Private undo entries now share immutable
snapshots through `Arc`. Reuse requires equality of the complete atom buffer
(including IDs), cursor, normalized selection and prompt history. Restoring a
shared snapshot clones its contents before editing. Public owned snapshot APIs,
delete grouping, redo invalidation and independently cloned editors are preserved.

This adds 38 production lines across two files, both under 500 lines. An existing
attachment/undo test now covers cursor, selection and history changes before an
edit, undo/redo restoration and a replacement branch. All six editor tests pass
before implementation. Temporarily dropping selection from snapshots makes that
extended test fail; the mutation was restored. All 1,668 TUI tests, seven gated
PTY tests, workspace compilation, Clippy, formatting and suite gates pass. The
six editor tests also pass after the final empty-stack guards. No snapshots or
backend files changed.

## Measurements

The unchanged release fixture alternates insertion/backspace into a fixed Unicode
draft, with zero history, ten warmup frames and 500 measured frames at 160×48.
It asserts actual input effects and the final draft. A short draft is the control.
The exact preceding executable and its unchanged fixture hash were retained.
Limits were recorded before implementation from its prior measured samples:
at least 35% less long-draft peak heap and 15% less RSS, with bounded timing and
allocation regressions. `compare.py` also enforces every stricter limit from the
preceding composer-text slice; the old short-draft p99 bound remains 179.3 µs.

Three uninstrumented timing samples and one separate glibc allocation sample per
side/scenario give these medians/totals:

| Long draft metric | Before | Candidate |
| --- | ---: | ---: |
| Peak heap bytes | 21,894,987 | 11,996,415 |
| RSS KiB | 40,608 | 25,248 |
| Allocated bytes | 372,704,799 | 362,809,358 |
| malloc calls | 5,440,069 | 5,243,984 |
| CPU ms/frame | 0.38 | 0.38 |
| p95 / p99 µs | 423 / 444 | 407 / 430 |

Peak heap falls 45.2%, RSS 37.8%, allocated bytes 2.7% and malloc calls 3.6%.
All fourteen effective limits pass in this paired run. Short-draft p95/p99 is
123/144 µs against 122/128 µs before, within the unchanged 135.3/179.3 µs bounds.
Earlier short-draft failures remain in composer-text evidence and their cause
is still unverified; this passing pair does not establish a lasting resolution.

Output bytes, visible content, history and frame counts match for every pair.
Timings include input handling, rendering, diffing and ANSI encoding to a sink;
they exclude PTY/emulator latency. Allocation totals cover the whole process.
Sharing removes adjacent duplicate ownership, while unique undo history remains
unbounded and equality traversal remains. No new CPU improvement is claimed.

## Terminal behavior and existing defect

Fifteen paired actual-runtime PTY/xterm captures cover the long Unicode draft,
insertion/deletion, middle edits, undo/redo, selection replacement/restoration,
Escape and collapsed/expanded paste. In each run, explicit assertions verify
redo restores the earlier screen and undo restores selection cells and cursor.
PNGs are byte-identical, and complete terminal snapshots match except observer
callback counts. Both runs exit naturally, restore termios and terminal modes,
and remove processes, sockets and browser profiles. The executables use the exact
predecessor and candidate with identical Chromium, xterm.js, font, reduced motion
and 140×40 geometry.

An initial predecessor attempt exposed an existing default Ctrl+Y conflict:
`KeyMap::default` binds it to Redo, then overwrites it with AllowPermission.
Ctrl+Y leaves the ordinary composer unchanged. The failed attempt is retained;
no production shortcut change was made. The final journey records that no-op on
both builds and uses the existing Ctrl+Shift+Z binding (CSI-u) to exercise redo.
Selection replacement uses one bracketed-paste event, matching one undo group.
This defect is deferred separately from the snapshot ownership change.

## Reproduction and limits

Run the ordinary checks with the commands recorded in `checks.json`, plus:

```sh
cargo nextest run --profile ci -p harness-tui --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

From a clean checkout of this change, copy `rebuild.py`, `measure.py`, `compare.py`
and `acceptance-before-implementation.json` to one scratch directory. Run
`rebuild.py` from the repository root. It builds both release probes and performance
executables, temporarily restoring two predecessor files and restoring the current
source in `finally`. Run `measure.py before candidate`, then `compare.py` from the
same root; the comparison fails on any original or current limit miss. Run:

```sh
node docs/evidence/tui-rewrite/undo-sharing/capture-composer.mjs SCRATCH/before-probe .omo/evidence/tui-rewrite/undo-sharing/reproduced/before
node docs/evidence/tui-rewrite/undo-sharing/capture-composer.mjs SCRATCH/candidate-probe .omo/evidence/tui-rewrite/undo-sharing/reproduced/candidate
python3 docs/evidence/tui-rewrite/undo-sharing/compare-browser.py .omo/evidence/tui-rewrite/undo-sharing/reproduced SCRATCH/browser-comparison.json
```

`performance.tar.gz` contains all sixteen raw runs, order and executable receipts.
`browser.tar.gz` contains terminal recordings, snapshots, PNGs and cleanup reports,
including the failed initial shortcut attempt. Source/file hashes and the patch
identify reviewed bytes. Original limits remain frozen. Whole TUI source is
166,953 lines, only 8.21% below the original; the 90,941-line target remains unmet.
The legacy state/text engines, sustained runtime targets, startup cadence, other
platforms and final whole-rewrite review remain unfinished. Existing CLI fixture
failures remain documented in reader-wake evidence; that suite was not rerun.

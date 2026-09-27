# Prompt editing controller

The predecessor is `172b776e6219c400005a43bd67d1f62e46099b48`. The replacement
controller scans borrowed prompt slices for word and line boundaries, removing
temporary character vectors. Shared cursor movement, recorded deletion and
undo/redo paths preserve selection anchors, newline deletion, mention adjustments
and overlay refresh order. The typed editor still gets the first undo/redo attempt,
followed by the existing fallback history. Public contracts, backend authority,
key bindings, geometry and timers are unchanged.

The controller shrinks from 293 to 227 lines, removing 66 production lines. Removing
385 private test/delegate lines and adding 150 integration/performance fixture
lines yields a net reduction of 301 TUI Rust lines. Whole `src` is 166,128 lines,
8.66% below the original; the frozen 90,941-line target remains unmet.
This replaces one private component; the old AppState, duplicate editor state
and fallback history remain.

## Behavioral coverage

Two public keyboard tests replace 26 private controller tests. Their cases cover
word/line/buffer movement, Unicode grapheme selection, existing selection anchors,
whole-line deletion including the newline, word and partial-line deletion,
undo/redo, selection replacement and history recall. The only deliberately retired
assertion inspects a private undo-stack length; storage policy is unchanged.
Existing atom-editor and actual-runtime journeys cover the typed editor path.

The public checks pass on the predecessor. A temporary predecessor mutation that
leaves the deleted line's newline behind fails the deletion check, then the
original source is restored. The replacement also passes all 1,660 TUI tests with the 26 old tests
still present. After review strengthened selection-anchor assertions, the final
public checks again pass on the predecessor. Clippy required splitting the same
cases into navigation/selection and deletion/undo/history checks, and checking
binding presence before dispatch in the helper. Preparation failures are retained.
All 1,635 final TUI tests, seven gated PTY checks, workspace compilation, Clippy,
formatting and suite gates pass. Commands
and statuses are recorded in `checks.json`.

Fixture preparation exposed an existing shortcut conflict: Ctrl+Home first binds
MoveBufferStart, then FirstMessage overwrites it. R19 records the unchanged
behavior. The public test assigns F12 to the configurable buffer-start action;
performance uses ordinary Home/End and the PTY journey uses ordinary Home. The earlier Ctrl+Y conflict
also remains unchanged, with Ctrl+Shift+Z exercising redo.

## Measurement protocol

Before changing production code, three release timing runs and one separate
glibc allocation run establish `acceptance-before-implementation.json`. The fixed
480-scalar Unicode draft has zero history, ten warmup frames and 500 measured
frames at 160×48. The cycle is Home, Ctrl+Right, End, Ctrl+Left. The frozen limits
require at least 400 fewer malloc calls across 510 actions, no allocated-byte
increase and bounded CPU, memory and tail latency. Earlier typing failures remain
open; this is a separate workload.

The baseline fixture checked unchanged text and final cursor. Review added
assertions after the timed interval for all four phases of the first measured
cycle, ensuring a missing End or Ctrl+Left cannot pass. Both final executables
use that same stronger fixture. The original four baseline runs and fixture hash
remain retained; inputs, warmups, timing boundaries and acceptance limits did not
change. A prior unmeasured binary using the conflicting Ctrl+Home is recorded as
preparation only.

The final pair uses three timing runs and one separate allocation run per side,
reversing side order for the middle timing sample. All seven frozen navigation
limits pass, with identical output bytes, content, frame counts and history.

| Metric | Before | Candidate |
| --- | ---: | ---: |
| p50 / p95 / p99 µs | 312 / 340 / 363 | 314 / 339 / 350 |
| CPU ms/frame | 0.32 | 0.32 |
| malloc calls | 4,632,701 | 4,632,184 |
| Allocated bytes | 328,147,888 | 326,443,366 |
| Peak heap bytes | 2,010,934 | 2,013,049 |
| RSS KiB | 9,592 | 9,808 |

The paired reduction is 517 malloc calls and 1,704,522 allocated bytes (0.52%).
CPU is unchanged; peak heap and RSS remain within the original bounds. The
measurement includes public input handling, rendering, diffing and ANSI encoding
to a sink, excluding PTY/emulator latency. Allocation totals cover the process.
This modest result does not establish the full rewrite's sustained CPU target.
All twelve runs are retained: four original baseline and eight final paired runs.

## Terminal protocol

The actual-runtime PTY/xterm journey extends the prior sixteen composer frames
with multiline word navigation, selection, deletion and undo/redo. It uses the
same emulator, font, theme, 140×40 geometry and reduced-motion setting for both
builds. Captures include complete cells/styles/cursor state, PNGs, raw ANSI,
inputs and cleanup reports. Only observer callback counters are excluded from
state equality; input comparison normalizes only the fresh workspace path. These
are settled-state checks, not new animation-cadence measurements.

The first predecessor preparation incorrectly expected a three-line paste to
collapse. Existing behavior displays three lines directly; collapse requires
four. Its failed marker wait, screenshot, raw recording and forced cleanup are
retained in `browser-preparation.tar.gz`. The corrected journey waits for the
visible three-line text and removes the unnecessary expand key. This changes
only the fixture. The four-line collapsed/expanded paste check remains.

All 23 final paired PNGs are byte-identical. Complete terminal state matches
except observer counts. Word-deletion undo restores the prior selection, and redo
restores the deletion. Both runs exit naturally, restore termios and terminal
modes, and clean up child processes, sockets and browser profiles.

## Reproduction

From a clean checkout of this change, copy `rebuild.py`, `measure.py`, `compare.py`
and `acceptance-before-implementation.json` to one scratch directory. From the
repository root run `python3 SCRATCH/rebuild.py`, then
`python3 SCRATCH/measure.py before candidate`, then `python3 SCRATCH/compare.py`.
Rebuild temporarily restores only the predecessor production controller and
restores current bytes in `finally`. Run builds before measuring, without other
builds or browser capture processes running. `red.py` reproduces the failing
predecessor mutation and restores the current controller in `finally`.

```sh
python3 docs/evidence/tui-rewrite/prompt-editing/checks.py
node docs/evidence/tui-rewrite/prompt-editing/capture-composer.mjs SCRATCH/before-probe .omo/evidence/tui-rewrite/prompt-editing/reproduced/before
node docs/evidence/tui-rewrite/prompt-editing/capture-composer.mjs SCRATCH/candidate-probe .omo/evidence/tui-rewrite/prompt-editing/reproduced/candidate
python3 docs/evidence/tui-rewrite/prompt-editing/compare-browser.py .omo/evidence/tui-rewrite/prompt-editing/reproduced SCRATCH/browser-comparison.json
```

`performance.tar.gz` contains the twelve raw reports, run order, logs, baseline
fixture patch and executable receipts. `browser.tar.gz` contains both final
recordings, complete cell snapshots and PNGs. Executables remain under
`/tmp/tui-prompt-editing`; source/file hashes identify the reviewed bytes.

Independent review verified the source/receipts, all twelve raw performance
reports, all 23 visual pairs and cleanup. It approved this bounded commit with
no blocking findings; `review.json` records the resolved coverage findings.

The full rewrite, source-reduction target, sustained-runtime CPU target, earlier
short-draft latency failures, startup cadence, other platforms and final whole
review remain unfinished. This component retains unique undo history and editor
mirrors. The full workspace test suite was not rerun; earlier predecessor CLI
fixture failures remain documented in reader-wake evidence.

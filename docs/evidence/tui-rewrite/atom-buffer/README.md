# Atom parsing and composer reflow

The predecessor is `7918123b2e2946a8b87a94a9c03fe9b594a13b58`.
Construction and insertion now share one parser over borrowed newline-separated
slices and a lazy grapheme iterator. This removes intermediate strings and
per-line grapheme vectors while preserving CRLF bytes, blank/trailing lines,
atom IDs and existing width rules. Invalid insertion still returns before mutation.

Atom validation uses a membership set instead of scanning every atom for every
ID. A reverse scan preserves the first conflicting input ID in the public error;
set iteration is never used. Reflow moves the validated atom list into its result
instead of cloning it twice. The bordered painter also passes its existing editor
model to the shared resolver, removing a second whole view-model construction.
Reflow replaces viewport-dependent rows at the same body width and row limit;
text, atoms, cursor and selection do not depend on the preparation viewport.

No public API, backend contract, width rule, snapshot or dependency changed.
All modified production files remain under 500 lines. The change removes 23 net
production lines and two test lines under `src`, while adding 22 net integration
test lines. Whole TUI source is 166,579 lines, only 8.41% below the original;
the 90,941-line target and full state/text replacement remain unfinished.

## Behavioral checks

Existing atom tests now cover CRLF/blank-line construction versus insertion and
unchanged state after invalid insertion. One new test protects duplicate-ID error
precedence, which the former quadratic scan made easy to change accidentally.
All eight pass before implementation. Temporary broken newline classification
and duplicate validation cause three expected failures; the source is restored.
All 1,659 final TUI tests, seven gated PTY checks, workspace compilation, Clippy,
formatting and suite gates pass. Existing recorded reference journeys remain
unchanged. `checks.json` contains the commands and statuses.

## Measurement sequence

The performance fixture is unchanged: a fixed long Unicode draft, alternating
insertion/backspace, zero history, ten warmup frames and 500 measured frames at
160×48, plus the short-draft control. It asserts real edit effects and final text.
The retained preceding release executable comes from `fdeee80b`; intervening
`7918123b` removes only the test-only stub and preserves its non-test prefix.
Exact source, fixture and executable hashes are retained.

Before editing, three timing runs and a separate glibc allocation run establish
`acceptance-before-implementation.json`. Prior stricter limits remain in force.
The first candidate passes its absolute limits, including CPU≤0.342 ms/frame,
but its paired CPU change is only 0.36→0.34 ms/frame (5.6%, one 10 ms clock tick).
Those sixteen paired runs and the first candidate receipt/source patch remain
under `step1` in the performance archive. No result is discarded.

Before reusing the already prepared editor model, `acceptance-second-step.json`
adds the stricter CPU≤0.324 ms/frame bound: 10% below that paired predecessor.
The final paired run uses three timing samples and one separate allocation run
per side and scenario, alternating order for the middle timing sample.

The first final pair and one unchanged confirmation pair give:

| Long draft metric | Before → candidate, first pair | Before → candidate, confirmation |
| --- | ---: | ---: |
| CPU ms/frame | 0.38 → 0.32 | 0.38 → 0.32 |
| malloc calls | 5,243,971 → 4,840,521 | 5,243,970 → 4,840,527 |
| Allocated bytes | 362,800,718 → 339,546,143 | 362,804,339 → 339,545,028 |
| Peak heap bytes | 11,994,299 → 11,966,486 | 11,996,411 → 11,964,374 |
| RSS KiB | 25,196 → 25,180 | 25,124 → 25,264 |
| p95 / p99 µs | 412 / 485 → 358 / 370 | 410 / 435 → 349 / 381 |

Long-draft CPU falls 15.8%, malloc calls 7.7% and allocated bytes 6.4% in both
pairs; peak heap and RSS are essentially unchanged. All seven long-draft bounds,
including CPU≤0.324 ms/frame, pass both times. Bytes, visible content, frame counts
and history match in every pair. Timings include public input handling, rendering,
diffing and ANSI encoding to a sink; they exclude PTY/emulator latency. Allocation
totals cover the whole process. These results do not establish the full rewrite's
30% sustained-runtime CPU target.

**The short-draft p99 gate fails in both final pairs.** The first pair is 138→196 µs
and confirmation 171→186 µs, above the unchanged 179.3 µs limit for the candidate.
Each final comparison therefore passes 13 of 14 limits and exits nonzero. Short
p95 is 121→127 and 126→123 µs; CPU is 0.10 ms/frame throughout. The baseline taken
before implementation also missed the earlier p99 bound (202 µs); its long p99
was 543 µs against 477.4 µs. No thresholds, warmups or measured samples were relaxed.

Several slow short-draft reports concentrate their tail in the first 12–32
measured frames, including one confirmation predecessor report. A separate
six-run diagnostic pins both unchanged executables to CPU 0: short median
p50/p95/p99 is 114/124/131 µs before and 118/128/142 µs after. The early slowdown
is absent there. This suggests sensitivity to execution conditions but does not
establish a cause or clear the original gate. CPU policy was read, not changed.
The 62 raw runs include eight baseline, sixteen first-step, sixteen final,
sixteen confirmation and six diagnostic runs. No further timing repetitions
were used to select a passing result.

## Terminal captures

Sixteen paired actual-runtime PTY/xterm captures preserve long Unicode editing,
undo/redo, selection restoration, Escape expiry/clear and collapsed/expanded
multiline paste. PNGs are byte-identical, and complete terminal snapshots match
apart from observer callback counts. Both runs exit naturally, restore termios
and terminal modes, and clean up child processes, sockets and browser profiles.
Emulator, font, geometry (140×40), reduced motion and keyboard inputs match.
Each run uses a fresh temporary workspace; the comparer checks all event fields
after normalizing only that recorded fixture path.
The existing Ctrl+Y conflict remains recorded; Ctrl+Shift+Z exercises redo.
These are settled-state checks, not new animation cadence measurements.

The initial fifteen-frame pair is retained separately. All PNGs match, but the
Escape frame's candidate snapshot still contains the 800 ms confirmation hint,
while its PNG and the predecessor snapshot contain the ordinary footer. The
capture helper takes the cell snapshot before the PNG; the timer can expire
between them. Both recordings retain the draft and contain the same confirmation
transition. The corrected fixture explicitly observes the hint and its expiry
before capture. It also sends two separately acknowledged CSI-u Escape events,
waiting for the draft text to disappear before the added capture. The captured
old text then returns as a dim italic history suggestion: the cursor is back at
the empty-prompt position, and the subsequent expanded paste contains only the
new text. Review caught the missing distinction in the original assertion. The
final comparer additionally checks those captured cursor/style changes and that
neither following paste frame contains the old draft. These assertions were added
against the retained raw captures, without changing input or production behavior. No production timer, key
binding, snapshot comparator or ignored field changed. Initial cell differences
are recorded in `browser-initial-differences.json`; their PNG and raw ANSI evidence
remain in `browser-initial.tar.gz`.

## Reproduction and remaining limits

Run the commands in `checks.json` and the full TUI check:

```sh
cargo nextest run --profile ci -p harness-tui --all-features
```

From a clean checkout of this change, copy `rebuild.py`, `measure.py`, `compare.py`,
`acceptance-before-implementation.json` and `acceptance-second-step.json` to one
scratch directory. From the repository root run `python3 SCRATCH/rebuild.py`,
`python3 SCRATCH/measure.py before candidate`, then `python3 SCRATCH/compare.py`.
Rebuild temporarily restores the eight predecessor source files and restores
current bytes in `finally`. Comparison enforces all original/current limits and
fails on a miss; the recorded final runs fail as disclosed above. Copy and run
`diagnose-affinity.py` only to reproduce the separate diagnostic, not acceptance.
`red.py` reproduces the deliberately broken predecessor with the retained tests
and restores current buffer source in `finally`.

```sh
node docs/evidence/tui-rewrite/atom-buffer/capture-composer.mjs SCRATCH/before-probe .omo/evidence/tui-rewrite/atom-buffer/reproduced/before
node docs/evidence/tui-rewrite/atom-buffer/capture-composer.mjs SCRATCH/candidate-probe .omo/evidence/tui-rewrite/atom-buffer/reproduced/candidate
python3 docs/evidence/tui-rewrite/atom-buffer/compare-browser.py .omo/evidence/tui-rewrite/atom-buffer/reproduced SCRATCH/browser-comparison.json
```

`performance.tar.gz` retains all raw samples, order, logs, intermediate source
patch and executable receipts; executables remain in `/tmp/tui-atom-buffer`.
`browser.tar.gz` contains the final terminal recordings, PNGs, full cell snapshots,
inputs, environment and cleanup reports. Source/file hashes identify reviewed
bytes. Prior CLI fixture failures remain documented in reader-wake evidence;
the full workspace test suite was not rerun for this TUI-only change.

The short-draft gate, unbounded unique undo history, duplicate editor state,
legacy state/text engines, source-reduction target, sustained runtime targets,
startup cadence, other platforms and final whole-rewrite review remain open.

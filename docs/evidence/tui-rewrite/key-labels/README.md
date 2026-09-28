# Shortcut label replacement

The predecessor is `6f04ab049bc178a26aee2b7cc828b4d75526e6ff`. Key labels now
format through `Display` on the existing `KeyBinding` type. The private
fragment-vector, temporary key string and join implementation is removed.
A borrowed iterator serves action lookups; public methods still return their
existing owned strings and vectors. Registration, overrides, parsing, routing
and backend contracts are unchanged.

Global and session bindings retain insertion order, followed by the existing
leader-map iteration. Primary labels prefer leader chords. Label lists still
sort and deduplicate bare leader suffixes. `all_bindings` excludes leaders and
sorts by displayed label, then action identifier. Literal characters,
Ctrl/Shift/Alt order, arrows, abbreviations, ignored Super/Hyper/Meta bits and
the repeated Shift in explicitly shifted BackTab labels remain unchanged.
Uncommon keys retain their debug spelling under a narrowly scoped Clippy
allowance with a documented reason.

Production falls by five lines. Two existing tests gain 34 lines, leaving the
complete TUI source tree at 165,676 lines. The 90,941-line target remains unmet.
The retained registry and test file still exceed 500 lines at 1,127 and 874.
No cache, dependency or production module is added.

## Behavioral evidence

The existing override-label test adds distinct modifier, leader, whitespace and
abbreviation cases. The binding-list check now verifies actual insertion order,
primary leader priority and sorted labels through public methods. All 40
keybinding checks pass on the predecessor. Swapping Control and Shift in the
old formatter makes the extended override check fail; 39 pass and one fails.
The replacement passes the full TUI suite without adding a test function.

A temporary differential test compares 28,480 labels with the source-extracted
predecessor formatter. It covers all 64 combinations of the six defined modifier
bits, every function-key byte, every ASCII character, selected Unicode scalars,
and all current named, media and modifier key variants. All labels match. The
temporary fixture is removed afterward; its generator, source and receipt remain.

All 1,635 TUI checks, seven gated PTY checks, workspace check, Clippy, formatting
and suite gates pass. The initial Clippy run rejected debug formatting inside
`Display`; the next required a formal reason on the allowance. Both failed
logs are retained in `preparation.tar.gz`. The allowance does not alter emitted
labels. The workspace test suite was not rerun; earlier predecessor CLI fixture
failures remain recorded.

## Measurement protocol

The preceding painter allocation trace identified repeated key-label string and
vector allocations. Its diagnostic timings are not used here. The predecessor
release benchmark and terminal probe are reused after verifying executable,
fixture and source receipts. Sixteen baseline runs precede production edits.
Frozen acceptance requires at least 10% fewer long-typing malloc calls and 5%
fewer allocated bytes, with no allocation/call increases elsewhere. All older
stricter latency, CPU and memory limits remain in effect.

Each of four workloads uses three timing runs and a separate `memusage` run:
500 frames, ten warmups, 160×48 geometry and no history. The middle paired sample
reverses executable order. Measurements run without builds, tests, profiling or
browser captures. Timings include input handling, frame preparation, rendering,
diffing and ANSI encoding; they exclude PTY and emulator delivery.

## Results

The candidate passes 26 of 28 frozen limits. Long typing malloc calls fall
14.6%, exceeding the required 10%. Allocated bytes fall 1.2%, missing the 5%
target: 16,643,206 bytes exceed the 15,994,884 bound. Short typing p99 is 186 µs,
above its frozen 139.7 µs bound and the paired predecessor's 174 µs. That
predecessor also exceeds the bound; the cause and stable end-to-end latency
remain unresolved. No limit changed and no unchanged rerun was used to obtain
passing results.

| Workload | p50/p95/p99 µs, before → candidate | CPU ms/frame | Allocation bytes | Malloc calls |
|---|---|---|---|---|
| Long typing | 218/238/247 → 216/236/247 | 0.22 → 0.22 | 16,840,480 → 16,643,206 | 94,699 → 80,857 |
| Short typing | 109/123/174 → 109/122/186 | 0.10 → 0.10 | 8,131,275 → 7,939,625 | 88,671 → 74,843 |
| Deep undo/redo | 233/255/266 → 231/250/261 | 0.22 → 0.22 | 38,919,562 → 38,725,416 | 396,885 → 383,051 |
| Grouped deletion | 297/358/572 → 297/353/367 | 0.30 → 0.30 | 52,967,027 → 52,770,801 | 618,022 → 604,183 |

All other latency, CPU, peak-heap and RSS bounds pass. CPU is unchanged at
process-tick resolution. Long peak heap is 2,195,428 → 2,193,319 bytes and RSS
is 9,652 → 9,624 KiB. All 16 paired output records retain exact workload,
history, frame count, ANSI byte count and visible/oldest text. All 48 raw
reports remain available, including baseline and failed paired measurements.

## Terminal evidence

The 31-frame composer journey covers Unicode, selection, undo/redo, paste,
resizing and word-wrap boundaries. An 11-frame journey covers deep undo, redo
branching and grouped backspaces. Predecessor captures are reused after checking
the probe hash and unchanged capture scripts; `browser-reference-reuse.json`
records every reused file hash.

All 42 pairs match exactly in PNG bytes, cells, styles and cursor. The initial
prompt/footer and multiline word-selection frames were inspected visually.
Inputs and emulator settings match, excluding only observer callback counts and
normalized temporary workspace paths. All four reports show natural process
exit, terminal restoration and process, socket, browser and temporary-directory
cleanup. Reduced-motion captures do not establish animation cadence or
end-to-end input latency.

## Reproduction

Copy `rebuild.py`, `measure.py`, `compare.py`, `checks.py` and
`acceptance-before-implementation.json` to a scratch directory. Use the source
and fixture versions recorded in the receipts. From the repository root:

```sh
python3 SCRATCH/rebuild.py
python3 SCRATCH/measure.py before candidate
python3 SCRATCH/compare.py
python3 SCRATCH/checks.py
```

Rebuild temporarily restores the predecessor `keybindings.rs`, then restores
current contents in `finally`. Do not edit or run other builds during that
operation. The recorded run reused the predecessor and built only the candidate
with `rebuild.py --candidate-only` after checking the reuse receipt. The default
command rebuilds both executables.

To reproduce the differential check, also copy `verify-labels.py`,
`legacy-keybindings.rs` and `oracle-cases.rs`, then run
`python3 SCRATCH/verify-labels.py`. It extracts the frozen formatter, installs
the temporary test, runs nextest and removes it in `finally`. `red.py` installs
the predecessor formatter with swapped modifier order and restores current
source afterward. Run it against the extended tests.

Browser captures use `capture-composer.mjs` and `capture-grouped.mjs` with
arguments `SCRATCH/before-probe EVIDENCE_DIR` and
`SCRATCH/candidate-probe EVIDENCE_DIR`. Put each pair under `before` and
`candidate` within an evidence directory below `.omo/evidence`. Run
`compare-browser.py PARENT OUTPUT_JSON` or `compare-grouped.py PARENT OUTPUT_JSON`.

`performance.tar.gz` holds all 48 raw reports: 16 baseline and 32 paired runs,
plus build metadata. `browser.tar.gz` holds PNGs, complete cell states, ANSI
streams and input/restoration/cleanup reports. `files.json` hashes every
published artifact except itself. Frozen source stays outside compiled
production code.

The full rewrite remains unfinished. The frame-layout and state engines,
public owned composer models, duplicate prompt state and fallback history
remain. Source reduction, sustained CPU, startup cadence, the remaining feature
and environment matrix, and final independent review still require work.

The independent reviewer approved this intermediate commit after verifying all
48 pre-review artifact receipts, source and executable reuse, raw performance
reports, frozen limits, formatter oracle, test logs, 42 terminal pairs and four
cleanup reports. `review.json` records the decision. It does not waive the two
failed limits or unfinished rewrite requirements.

# Composer row measurement replacement

The predecessor is `b832c3ca0afbe2ec71e473ac63c744f8abdc83a8`. Atom wrapping now
yields borrowed atom slices and stored row widths. Public `AtomBuffer::wrap`
materializes the same owned ID rows. Runtime presentation counts only its visible
budget; completion geometry counts every row. Neither count builds ID vectors.

Frame sizing shares the string viewport's soft-wrap primitive and stops at its
row cap. Its old per-grapheme vector and second wrapping algorithm are removed.
The hard-break policies remain outside the shared helper: frame sizing splits
every LF, while the viewport splits standalone LF graphemes. R21's public-mirror
CRLF discrepancy remains. Atom widths, atomic attachments and string grapheme
widths retain their distinct contracts. Coordinator and backend code is unchanged.

## Behavioral evidence

The existing atom-wrap test now checks exact row IDs and widths for zero-width
atoms, an over-wide first atom, width zero, saturation, newline ownership and a
trailing empty row. Public stored widths remain authoritative; newline widths
are ignored. The existing completion test includes more rows than its visible
cap and checks that `wrapped_lines` still counts the hidden rows.

All 19 checks in the three affected integration binaries pass on the predecessor.
A deliberate predecessor mutation treats a nonempty row as having positive
width; the extended atom test fails because a leading zero-width attachment
incorrectly becomes a separate row. The other 18 checks pass. All 19 pass with
the replacement. No permanent test function is added.

Three temporary differential checks compare 174,696 complete viewports, 2,431
atom-row results and 45,135 frame heights with frozen implementations. All
222,262 results match exactly. The viewport oracle retains the earlier
231-line implementation; atom wrapping and frame-height references come from
this step's predecessor. `make-oracles.py` extracts the current height functions
and includes current viewport/text source, so it cannot silently test a stale
copy of the replacement. Both temporary test files are removed from the normal
test tree; their checked source and reproduction scripts remain here.

Production source falls by 45 lines to 166,058. The two existing tests grow by
65 lines in total. The retained frame-layout file remains 1,306 lines; this step
replaces its composer measurement, not the whole layout engine. The 90,941-line
target remains unmet.

All 1,635 TUI checks, seven gated PTY checks, workspace check, Clippy, formatting
and suite gates pass. The workspace test suite was not rerun; earlier predecessor
CLI fixture failures remain recorded.

## Measurement protocol

The preceding step's candidate executable is reused after checking binary,
fixture and source hashes. `before-reuse-origin.json` retains its original build
receipt; `before/binary.json` in the performance archive records this step's six
source files and identifies the reuse. The candidate is built after quality
checks pass, with the same release profile and unchanged performance fixture.

Sixteen baseline runs precede production changes. Frozen limits require at least
10% fewer long-typing allocation bytes and malloc calls, no increases in either
for other workloads, and preserve older stricter latency, CPU and memory bounds.
Four workloads each use three timing runs and one separate `memusage` run:
500 frames, ten warmups, 160×48 geometry and no history. The middle paired sample
reverses executable order. No builds, tests or browser captures run alongside
measurements. Timings include input handling, frame preparation, rendering,
diffing and ANSI encoding; they exclude PTY and emulator delivery.

## Results

The candidate passes 25 of 28 frozen limits. Long typing allocates 72.6% fewer
bytes, but malloc calls fall only 5.4%, missing the 10% target. Short typing
p95/p99 exceed their limits. No limit changed and no unchanged rerun was used
to obtain a passing result.

| Workload | p50/p95/p99 µs, before → candidate | CPU ms/frame | Allocation bytes | Malloc calls |
|---|---|---|---|---|
| Long typing | 216/236/246 → 219/243/363 | 0.22 → 0.22 | 64,343,806 → 17,617,676 | 104,412 → 98,782 |
| Short typing | 106/121/185 → 111/161/195 | 0.10 → 0.10 | 8,437,412 → 8,186,458 | 92,743 → 90,186 |
| Deep undo/redo | 232/251/264 → 233/253/267 | 0.22 → 0.22 | 86,426,000 → 39,696,390 | 406,605 → 400,956 |
| Grouped deletion | 299/371/402 → 296/357/378 | 0.30 → 0.30 | 123,737,193 → 54,366,167 | 630,779 → 624,418 |

The failed bounds are 93,979.8 long-typing malloc calls and 133.1/179.3 µs short
p95/p99. Long p99 also rises substantially, though it stays below its frozen
401.5 µs limit. Individual timing samples vary; their causes remain unresolved.
CPU is unchanged at process-tick resolution across all four workloads. This
comparison establishes allocation savings, not improved responsiveness.

Undo and deletion allocation bytes fall 54.1% and 56.1%. All peak-heap and RSS
bounds pass. Long typing peak heap is 2,193,319 → 2,193,322 bytes and RSS is
9,740 → 9,924 KiB. All 16 paired output records retain exact workload, history,
frame count, ANSI byte count and visible/oldest text. All 48 raw reports remain
available, including the baseline and failed paired measurements.

## Terminal evidence

The 31-frame composer journey covers Unicode, selection, undo/redo, paste,
resizing and word-wrap boundaries. An 11-frame journey covers deep undo, redo
branching and grouped backspaces. The predecessor captures are reused from the
preceding candidate after verifying its probe hash and unchanged capture scripts.
`browser-reference-reuse.json` records every reused file hash.

All 42 paired frames match exactly in PNG bytes, cells, styles and cursor.
Narrow Unicode wrapping and the newline-after-dropped-space frame were inspected
visually. Inputs and emulator settings match; only observer callback counts are
excluded, and temporary workspace paths are normalized in input receipts.
All four reports show natural process exit, terminal restoration and process,
socket, browser and temporary-directory cleanup. These reduced-motion captures
do not establish animation cadence or end-to-end input latency.

## Reproduction

Copy `rebuild.py`, `measure.py`, `compare.py`, `checks.py` and
`acceptance-before-implementation.json` into a scratch directory. Use the source
and fixture versions recorded in the receipts. From the repository root:

```sh
python3 SCRATCH/rebuild.py
python3 SCRATCH/measure.py before candidate
python3 SCRATCH/compare.py
python3 SCRATCH/checks.py
```

Rebuild temporarily restores the six predecessor source files and restores
current bytes in `finally`. Do not edit or run other builds during that operation.
For differential checks, also copy `make-oracles.py`, `verify-geometry.py`,
`viewport-oracle.rs`, `oracle-cases.rs`, `legacy-buffer.rs` and `legacy-layout.rs`.
Run `python3 SCRATCH/verify-geometry.py`; it generates both temporary tests,
runs nextest and removes them in `finally`. `red.py` restores all six predecessor
sources before mutation and restores current bytes afterward. Extract the
performance archive first so its predecessor receipt is available to that script.

Browser captures use `capture-composer.mjs` and `capture-grouped.mjs` with
arguments `SCRATCH/before-probe EVIDENCE_DIR` and
`SCRATCH/candidate-probe EVIDENCE_DIR`. Place each pair under `before` and
`candidate` within an evidence directory below `.omo/evidence`. Run
`compare-browser.py PARENT OUTPUT_JSON` or `compare-grouped.py PARENT OUTPUT_JSON`.

`performance.tar.gz` contains all 48 raw reports: 16 baseline and 32 paired runs,
plus build metadata. `browser.tar.gz` contains PNGs, complete terminal states,
ANSI streams and input/restoration/cleanup reports. `files.json` hashes every
published artifact except itself. Frozen implementations are evidence outside
compiled production code.

The frame-layout engine, public owned composer models, duplicate prompt state
and fallback history remain. The complete rewrite, source-reduction target,
sustained CPU target, startup cadence, remaining feature/environment coverage
and final independent review remain open.

The independent reviewer approved this intermediate commit after checking all
58 pre-review artifacts, source receipts, 48 performance reports and 42 terminal
pairs. `review.json` records the decision. It does not waive the three failed
limits, the long-p99 increase or the unfinished rewrite requirements.

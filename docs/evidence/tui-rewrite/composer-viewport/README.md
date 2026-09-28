# Composer viewport replacement

The predecessor is `9681f5322d479a9d819af7fac09e5dfbfa053350`. The string
composer layout now discovers hard line breaks and wraps borrowed slices. It
stores only row slices and absolute scalar starts, then allocates strings for
the visible rows. The old per-grapheme records, all-row strings and visible-row
clones are removed. Public state, coordinator contracts and presentation policy
do not change. Atom-based row budgets remain separate from painted geometry.

## Geometry contract

The replacement preserves the first over-wide grapheme, word breaks including
the space, and a dropped overflowing space when no earlier break is available.
A leading space alone does not create a word break. Only a standalone LF grapheme
creates a hard line break. CRLF stays one whitespace grapheme in this layout.
Trailing LF adds an empty row; dropped whitespace immediately before LF does
not add an intermediate row.

Interior cursor columns use each grapheme's width with a one-cell minimum.
LF and end-of-text positions use the actual painted row width. A cursor inside
a grapheme advances past it. A dropped whitespace cluster maps its first scalar
to the preceding row, while an interior scalar can have no cursor. Soft-wrap
boundaries belong to the following row. Missing and out-of-range cursors anchor
the viewport to the tail. The total row count keeps following rows visible when
the cursor is near the beginning. Absolute scalar starts still drive pointer
mapping, mentions and selection painting.

Public prompt mirrors expose an existing CRLF discrepancy. For `one\r\ntwo`,
row budgeting splits at LF while painting retains the CRLF grapheme and shows
`onetwo` plus an empty row. R21 records preservation of that behavior. No shipped
keyboard sequence reproducing raw CRLF in the mirror has been established.

## Behavioral evidence

The existing public keyboard/composer check adds eight frame-and-cursor cases.
All five checks in its integration binary pass on the predecessor. A deliberate
mutation allowing a word break at the first space fails the leading-space case;
four checks pass and one fails. Restoring the replacement makes all five pass.
No new permanent test function or fixture is added. Clippy initially rejected
the nested cursor loop. Extracting that calculation into a small helper fixes
the nesting without changing the geometry; the differential check passes again.
The preliminary release build and its receipt are retained as unmeasured
preparation, separate from the final candidate.

A temporary differential fixture compares the complete viewport against a frozen
copy of the predecessor, including row text, scalar starts and cursor. It covers
174,696 combinations of short text, Unicode, controls, whitespace, LF and CRLF,
widths 0/1/2/3/5/10, row budgets 0/1/2/4, every scalar cursor, no cursor and an
out-of-range cursor. Hand-picked longer strings include dropped-space/LF pairs,
regional indicators and a family emoji. Both the predecessor and replacement
pass. The fixture is removed from the normal test tree after verification and
published here with a runnable reproduction script. The initial fixture compile
error is retained. Existing full-frame selection and mouse checks remain.

The viewport engine shrinks from 231 to 111 lines, and deleting its old visual
character type and alias removes nine more lines. Production source falls by
129 lines. TUI `src` totals 166,103 lines, still above the 90,941 target. The
existing public test grows by 72 lines. All modified source and test files are
under 500 lines.

All 1,635 TUI checks, seven gated PTY checks, workspace check, Clippy, formatting
and suite gates pass. The workspace test suite was not rerun; earlier predecessor
CLI fixture failures remain recorded.

## Measurement protocol

The previous step's candidate is the retained predecessor executable. Its source,
fixture and binary hashes are checked before reuse. `before-reuse-origin.json`
preserves the original receipt; the current receipt identifies this step's
predecessor and the two replaced source files without claiming a new build.

Sixteen baseline runs precede production changes and set frozen acceptance limits.
The four workloads each use three release timing runs and a separate `memusage`
run. Inputs, 500 frames, ten warmups and 160×48 geometry are unchanged. The middle
sample reverses binary order. Measurements run without concurrent builds, tests
or browser captures. Long typing must reduce allocation bytes and malloc calls
by at least 10%; other workloads cannot increase either. Older stricter latency,
CPU and memory bounds remain. Timings include input handling, frame preparation,
rendering, diffing and ANSI encoding, and exclude PTY and emulator delivery.

## Results

The candidate passes 27 of 28 frozen limits. Long typing allocates 12.5% fewer
bytes, but malloc calls fall only 2.4%, short of the required 10%. Its 104,409
calls exceed the 96,273 bound. That target remains unmet. No acceptance limit
changed and no unchanged rerun was used to obtain a passing result.

| Workload | p50/p95/p99 µs, before → candidate | CPU ms/frame | Allocation bytes | Malloc calls |
|---|---|---|---|---|
| Long typing | 220/240/244 → 216/237/243 | 0.22 → 0.22 | 73,548,142 → 64,343,172 | 106,974 → 104,409 |
| Short typing | 111/128/200 → 109/122/127 | 0.12 → 0.10 | 9,024,948 → 8,436,330 | 93,761 → 92,751 |
| Deep undo/redo | 236/254/265 → 234/254/266 | 0.24 → 0.24 | 95,630,200 → 86,421,622 | 409,157 → 406,593 |
| Grouped deletion | 307/373/391 → 301/367/392 | 0.30 → 0.30 | 137,372,201 → 123,736,095 | 633,617 → 630,786 |

Long, undo and deletion CPU are unchanged at process-tick resolution. Short
latency and CPU meet their limits in this comparison; earlier failed comparisons
remain recorded and this does not establish stable end-to-end latency. Undo and
deletion p99 each rise by 1 µs within their bounds. Peak heap and RSS stay within
all bounds. Long typing peak heap is 2,193,323 → 2,193,326 bytes and RSS is
9,896 → 9,824 KiB. All 16 paired output records retain exact scenario, frame
count, history, ANSI byte count and visible/oldest text.

The performance archive contains all 48 reports, comprising 16 baseline runs
and 32 paired runs. The preliminary binary was not measured; its failed lint
record and source-matched build receipt stay in the preparation archive.

## Terminal evidence

The 31-frame composer journey adds word-wrap boundary editing at 20×24 to the
preceding Unicode, selection, undo/redo, paste and resize journey. A separate
11-frame journey covers deep undo, redo branching and grouped backspaces. Its
predecessor capture is reused from the prior candidate with the matching binary
hash. All 42 paired frames match exactly in PNG bytes, cells, styles and cursor.
The space-boundary, leading-space and newline-after-space frames were also
inspected visually. Inputs and emulator settings match on both sides. Only observer callback
counts are excluded from equality; temporary workspace paths are normalized in
input receipts. The reports retain terminal restoration and process, socket,
browser and temporary-directory cleanup checks. Reduced-motion captures do not
establish animation cadence or end-to-end input latency.

## Reproduction and remaining work

Copy `rebuild.py`, `measure.py`, `compare.py`, `checks.py` and
`acceptance-before-implementation.json` to a scratch directory. Use the source
and performance fixture versions recorded in the receipts. Run from the repo
root:

```sh
python3 SCRATCH/rebuild.py
python3 SCRATCH/measure.py before candidate
python3 SCRATCH/compare.py
python3 SCRATCH/checks.py
```

Rebuild temporarily restores the two predecessor source files and restores the
current bytes in `finally`. Do not edit or run other builds during that operation.
For the differential check, also copy `differential.rs` and `verify-geometry.py`,
then run `python3 SCRATCH/verify-geometry.py`. It installs the temporary fixture,
runs nextest and removes it in `finally`. `red.py` restores both predecessor source files before mutation and restores
current source afterward. The initial runner is retained separately.

Browser captures use the `capture-composer.mjs` and `capture-grouped.mjs` scripts
with arguments `SCRATCH/before-probe EVIDENCE_DIR` and
`SCRATCH/candidate-probe EVIDENCE_DIR`. Put the two runs under `before` and
`candidate` within an evidence directory below `.omo/evidence`. Run
`compare-browser.py PARENT OUTPUT_JSON` for the composer pair and
`compare-grouped.py PARENT OUTPUT_JSON` for the grouped pair.

Extract `performance.tar.gz` for raw samples and build metadata,
`browser.tar.gz` for PNGs, cell states, ANSI streams and input/cleanup reports,
and `preparation.tar.gz` for the initial lint failure and unmeasured build.
`files.json` hashes every published artifact except itself. The old viewport
source appears only in the independent evidence fixture, outside compiled code.

The frame-layout engine, atom row-budget implementation, public owned composer
models, duplicate prompt state and fallback history remain. The full rewrite,
source-reduction target, sustained CPU target, startup cadence and remaining
environment/feature verification still require work.

The independent reviewer approved this intermediate commit after verifying all
62 pre-review artifact receipts, three source receipts, four executable hashes,
48 performance reports and 42 terminal pairs. `review.json` records the decision.
It does not waive the malloc-count failure or approve the full rewrite.

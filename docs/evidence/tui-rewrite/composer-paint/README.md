# Composer painting replacement

The predecessor is `03784425760ff676c37d9cb8f487ca03106b6cd2`. Composer style runs
now borrow their text slices. The bordered painter borrows plain text and its
measured prefix/gutter, and adds those spans to the existing body span vector.
It keeps `Paragraph` rendering, clipping, ghost text, cursor placement and
disabled/placeholder/collapsed-paste policy. Backend contracts are unchanged.

## Removed implementation

The old `document.rs` painter had one caller. That dispatcher returns separately
for `ReplayReadOnly`; the only remaining variants are `Startup` and `Live`, both
of which immediately delegated to the bordered painter. Its fallback and the
175-line metadata module used only by that fallback are removed. The dispatcher
now calls the bordered painter directly. The private context loses the obsolete
disclosure field and document-specific name.

The two existing `composer_metadata_*` checks remain: they exercise live badge
rendering and provider display labels. The removed private metadata helper had
no tests; its test-only re-export was unused. Identity, disclosure and shared
color helpers retain live callers and remain in production.

Production source falls by 443 lines. The existing painted interaction test
grows by 32 lines, leaving the complete TUI `src` tree at 165,647 lines. The
90,941-line target remains unmet. The touched legacy dispatcher and test file
remain 1,554 and 827 lines; the active bordered painter is 344 lines.

## Behavioral evidence

Tag membership still uses a grapheme's starting scalar; selection uses scalar
overlap. Combined selection keeps the tag's warning color and bold modifier.
Adjacent runs merge only when their complete styles match. Empty lines retain
one empty span with the base style. The continuation gutter uses the measured
prefix width, including custom glyph widths.

The existing painted mention/selection journey adds combined-style, partial
grapheme and unselected-boundary assertions. All eight affected checks pass on
the predecessor. A mutation replacing the selected style instead of adding
reverse loses the mention color and fails the extended check; seven pass and
one fails. The replacement passes. No permanent test function is added.

A temporary independent oracle compares complete `Line` values, including span
boundaries and styles, with the frozen predecessor. All 289,152 combinations of
Unicode, controls, whitespace, scalar offsets, overlapping tags, selection
ranges and base/tag styles match. The source-extracted functions use a fixture
tag with the same two scalar-bound fields they consume. The public painted
journey exercises the real tag type. The temporary test is removed afterward.

The first full TUI run passed, but Clippy rejected the extended test function's
cognitive complexity. Moving the new assertion stage into a small helper fixes
that limit without changing the assertions. The predecessor mutation is rerun
after that edit. The failed lint log and initial test source remain in
`preparation.tar.gz`; no release candidate was built before that gate passed.

All 1,635 TUI checks, seven gated PTY checks, workspace check, Clippy, formatting
and suite gates pass. The workspace test suite was not rerun; earlier predecessor
CLI fixture failures remain recorded.

## Measurement protocol

A separate symbolized release build sampled every 200th malloc call through a
GDB runner under nextest. Of 494 sampled stacks, 15 first reached production at
the style-run string append and 14 at the extra bordered-row span vector.
Repeated runtime-state and keybinding formatting produced more samples and
remain future work. The diagnostic includes setup and warmup, excludes
calloc/realloc, and changes execution timing. Its timings are not acceptance
evidence. The initial stripped-binary attempt could not resolve source frames;
both attempts are retained in `diagnostic.tar.gz`.

The normal predecessor executable is reused after checking binary, fixture and
source hashes. It is separate from the diagnostic build. Sixteen baseline runs
precede production changes. Frozen limits require at least 5% fewer long-typing
allocation bytes and malloc calls, no allocation/call increases in other
workloads, and keep every older stricter latency, CPU and memory limit.

Each of four workloads uses three timing runs and a separate `memusage` run:
500 frames, ten warmups, 160×48 geometry and no history. The middle paired sample
reverses executable order. Measurements run without builds, tests, profiling or
browser captures. Timings include input handling, frame preparation, rendering,
diffing and ANSI encoding; they exclude PTY and emulator delivery.

## Results

The candidate passes 26 of 28 frozen limits. Long typing allocation bytes fall
4.5% and malloc calls 4.2%, both short of the required 5%. Its 16,830,430 bytes
exceed the 16,740,421.2 bound; 94,684 calls exceed 93,842.9. No bound changed and
no unchanged rerun was used to obtain passing results.

| Workload | p50/p95/p99 µs, before → candidate | CPU ms/frame | Allocation bytes | Malloc calls |
|---|---|---|---|---|
| Long typing | 217/237/249 → 213/229/246 | 0.22 → 0.22 | 17,627,120 → 16,830,430 | 98,796 → 94,684 |
| Short typing | 110/124/139 → 109/121/128 | 0.10 → 0.10 | 8,193,342 → 8,125,449 | 90,204 → 88,656 |
| Deep undo/redo | 232/251/263 → 229/245/261 | 0.22 → 0.22 | 39,699,698 → 38,908,448 | 400,963 → 396,866 |
| Grouped deletion | 295/357/376 → 292/351/378 | 0.30 → 0.30 | 54,363,051 → 52,962,865 | 624,411 → 618,011 |

All latency, CPU, peak-heap and RSS bounds pass in this comparison. Earlier
failed latency comparisons remain recorded; their causes and stable end-to-end
latency remain unresolved. CPU is unchanged at process-tick resolution. Deletion
p99 rises by 2 µs within its bound. Long peak heap is 2,195,432 → 2,193,323 bytes
and RSS is 9,744 → 9,864 KiB. All 16 paired output records retain exact workload,
history, frame count, ANSI byte count and visible/oldest text. All 48 raw reports
remain available, including baseline and failed paired measurements.

## Terminal evidence

The 31-frame composer journey covers Unicode, selection, undo/redo, paste,
resizing and word-wrap boundaries. An 11-frame journey covers deep undo, redo
branching and grouped backspaces. Predecessor captures are reused after checking
the probe hash and unchanged capture scripts; `browser-reference-reuse.json`
records every reused file hash.

All 42 pairs match exactly in PNG bytes, cells, styles and cursor. The word
selection and collapsed-paste frames were inspected visually. Inputs and emulator
settings match, excluding only observer callback counts and normalized temporary
workspace paths. All four reports show natural process exit, terminal restoration
and process, socket, browser and temporary-directory cleanup. Reduced-motion
captures do not establish animation cadence or end-to-end input latency.

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

Rebuild temporarily restores the seven predecessor files, then restores current
contents or absence in `finally`. Do not edit or run other builds during that
operation. To reproduce the differential check, also copy `verify-geometry.py`,
`legacy-file_tags.rs` and `oracle-cases.rs`, then run
`python3 SCRATCH/verify-geometry.py`. It extracts current styling source, installs
the temporary test, runs nextest and removes it in `finally`. `red.py` restores
all seven predecessor files before mutation and restores current contents or
absence afterward. Extract the performance archive first for its receipt.

Browser captures use `capture-composer.mjs` and `capture-grouped.mjs` with
arguments `SCRATCH/before-probe EVIDENCE_DIR` and
`SCRATCH/candidate-probe EVIDENCE_DIR`. Put each pair under `before` and
`candidate` within an evidence directory below `.omo/evidence`. Run
`compare-browser.py PARENT OUTPUT_JSON` or `compare-grouped.py PARENT OUTPUT_JSON`.

`performance.tar.gz` holds all 48 raw reports: 16 baseline and 32 paired runs,
plus build metadata. `browser.tar.gz` holds PNGs, complete cell states, ANSI
streams and input/restoration/cleanup reports. The diagnostic archive contains
the GDB scripts, sampled stacks and separate symbolized build receipt; its
absolute scratch paths need updating when reproduced elsewhere. `files.json`
hashes every published artifact except itself. Frozen source stays outside
compiled production code.

The diagnostic build uses predecessor source and the preceding composer-rows
scratch metadata. `profile-command.json` records its exact nextest runner
invocation and environment. `profile-summary.py` recomputes the source-location
counts from the retained stacks.

The full rewrite remains unfinished. The frame-layout and state engines, public
owned composer models, duplicate prompt state and fallback history remain.
Source reduction, sustained CPU, startup cadence, the remaining feature and
environment matrix, and final independent review still require work.

The independent reviewer approved this intermediate commit after checking all
52 pre-review artifact receipts, source/deletion receipts, 48 raw performance
reports, 42 terminal pairs and four cleanup reports. `review.json` records the
decision. It does not waive the two allocation failures, earlier latency
uncertainty or unfinished rewrite requirements.

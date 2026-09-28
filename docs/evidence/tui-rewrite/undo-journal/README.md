# Undo journal

The predecessor is `68dd6c27462ed2435259b441bb06cf01eff7fd99`. The atom editor's
undo storage now keeps a complete state at each stack's tip and records only the
changed atom range, cursor, selection and changed history for earlier entries.
A bridge records changes between edits, such as cursor movement or history state.
It restores the previous tip before traversing the next entry. Empty stacks drop
their tips. Immutable entries share their payloads when an editor is cloned.

The journal preserves complete atoms, IDs, display widths and the private next-ID
allocator. It does not rebuild snapshots through `from_atoms`, which would lose
allocator history after deletion. Character deletions still coalesce only across
identical complete snapshots. Equal before/after records preserve redo. Public
owned `EditorSnapshot` and `UndoStack` APIs retain their behavior and equality,
including redo's stored current value even when restoration does not read it.

Recording temporarily moves the editor's buffer and history into the existing
snapshot type, borrows it synchronously, then restores those fields. The journal
updates its uniquely owned tip with a forward patch; it avoids cloning and freeing
a complete prompt on every ordinary edit. Shared tips copy on mutation, preserving
independent editor branches. This replaces the old full-snapshot storage engine;
it introduces no dependency or generic text-storage framework.

## Behavioral checks

One public undo-stack sequence covers gaps between records, differing current
values, grouping, exact metadata and allocator restoration, no-op redo preservation,
redo's future undo target, public equality and branch invalidation. An existing
editor test now compares complete atom state after grouped deletion and after
subsequent insertion. Existing editor-clone and selection checks remain.
All seven editor checks pass on the predecessor. A deliberately broken redo
implementation installs the wrong future undo target: the new public check fails,
while the other six pass. The source is restored before replacement.

The public contract is unusual but preserved: `record(A,B); undo(C)` returns A;
`redo(D)` returns B; the next undo returns D. The journal retains these separate
values, including unrecorded cursor boundary, empty selection and history changes.

## Measurement protocol

The retained release executable is the preceding committed implementation.
An initial eight-run typing baseline uses its unchanged fixture. A new deep-undo
scenario then builds 500 alternating insertion/backspace edits into the fixed
Unicode draft, performs ten alternating undo/redo warmups, and measures 250 undos
followed by 250 redos. Long- and short-draft typing retain their preceding inputs.
All scenarios use zero transcript history, 500 measured frames and 160×48 geometry.

Twelve baseline runs with the new scenario establish the limits before production
changes: at least 50% lower peak heap and 25% lower RSS for long typing and deep
undo, plus at least 2% fewer long-typing allocation bytes and malloc calls. Prior
stricter typing limits remain, including CPU≤0.324 ms/frame and short p99≤179.3 µs.
The new deep-undo scenario has its own latency, CPU and allocation bounds. The
long-typing baseline itself measures 0.34 ms/frame, exceeding that older CPU limit;
the bound is not relaxed. The original eight and these twelve runs remain retained.

Review strengthened the deep-undo fixture to check the last two undo and redo
steps as well as the first two. Repeated insertion/backspace returns to the same
text, so a shortened history could otherwise exhaust early and pass the final
text assertion. The checks are outside timed samples; both final binaries use
the same strengthened fixture. Inputs, warmups and frozen limits are unchanged.
The original baseline fixture patch and hash are retained.

Independent review also identified grouped deletion as a possible regression:
coalescing reconstructs the original draft and copies the growing deleted range.
The added `delete-long` scenario applies ten warmup and 500 measured backspaces
to 768 Unicode graphemes, then checks the exact remaining prefix. Four predecessor
runs establish seven additional limits before candidate measurement or any change
to grouping. Those limits allow 10% growth, with a p99 allowance of the larger of
100 µs or 10%. Existing limits are unchanged. No grouping optimization was added.

Each workload has three timing runs and one separate glibc `memusage` run per
binary. The middle timing run reverses binary order. Builds, tests and browser
captures do not overlap measurements. Timing includes public input handling,
rendering, Ratatui diffing and ANSI encoding into a counting sink; it excludes PTY
delivery and terminal-emulator latency. CPU uses Linux process ticks, so 0.02 ms
per frame is one tick across this 500-frame workload.

## Results

The final four-workload comparison passes 26 of 28 frozen limits. Short-draft
p95 is 140 µs against 135.3 µs, and p99 is 197 µs against 179.3 µs. The predecessor
measures 119/125 µs in this pair. The preceding three-workload comparison passed
all 21 limits, but does not clear these later failures. Earlier short-draft
failures remain unresolved; this evidence does not establish their cause.

| Workload | p95 µs, before → journal | p99 µs | CPU ms/frame | RSS KiB | Peak heap bytes |
|---|---:|---:|---:|---:|---:|
| Long typing | 353 → 328 | 366 → 349 | 0.32 → 0.32 | 25,176 → 9,964 | 11,964,372 → 2,248,087 |
| Short typing | 119 → 140 | 125 → 197 | 0.10 → 0.12 | 9,676 → 9,884 | 2,023,491 → 2,115,526 |
| Deep undo/redo | 322 → 341 | 345 → 363 | 0.32 → 0.32 | 24,988 → 10,092 | 11,757,517 → 2,211,632 |
| Grouped deletion | 540 → 557 | 571 → 580 | 0.40 → 0.44 | 10,080 → 9,852 | 2,045,277 → 2,055,032 |

Long typing reduces peak heap by 81.2%, RSS by 60.4%, allocated bytes by 2.85%
and malloc calls by 4.0%. Deep undo/redo reduces peak heap by 81.2% and RSS by
59.6%, while allocated bytes rise 0.08%. Grouped deletion allocates 4.4% more
bytes and takes 10% more CPU. Its last 100 actions have median latency 319 µs
versus 275.5 µs, a 15.8% increase; aggregate deletion gates still pass. This
coalescing cost remains a documented limitation, not a resource improvement.

`performance.tar.gz` retains all 80 raw reports: eight initial typing runs,
twelve baseline runs, 24 first paired runs, four deletion baseline runs and
32 final paired runs. Each stage retains binary/fixture hashes and run order;
the baseline includes its fixture patch. `summary.json`, `comparison.json` and
`deletion-phases.json` describe the final run. Local release executables remain
under `/tmp/tui-undo-journal`; `rebuild.py` reproduces them from source.

All 1,636 TUI checks, seven gated PTY checks and workspace check, Clippy, format
and suite gates pass. The final performance fixture also passes Clippy and
format checks. The workspace test suite was not rerun; earlier predecessor CLI
fixture failures remain recorded. Preparation failures and the corrected red
mutation are retained. This change adds 120 production lines across four files;
the longest is 232 lines. Whole TUI source is 166,248 lines, 8.60% below the
original 181,882 and above the 90,941 target.

## Terminal comparisons

The new eight-frame actual-runtime PTY/xterm journey starts with the long Unicode
draft, makes 120 distinct typed edits, undoes 110 and redoes 110. It also checks
that a new branch invalidates redo. Explicit text counts distinguish real deep
traversal from an exhausted history. The existing 23-frame composer journey adds
selection restoration, word/line movement and deletion, Escape clearing, and
collapsed/expanded paste. Its predecessor capture is reused from prompt-editing:
the recorded executable hash matches this change's retained reference exactly.
An extended 11-frame version also applies 80 consecutive backspaces, restores
them with one undo and repeats them with one redo. All 11 pairs and the 23
composer pairs match exactly in PNGs and full terminal state. The initial eight
deep-undo pairs are retained too. No reference cells, styles, PNGs, inputs or
expected snapshots are changed. The deletion and restored-draft PNGs were also
visually inspected.

These are settled-state comparisons with matching emulator, font, geometry,
theme and reduced motion. Only observer callback counts are excluded from full
terminal-state equality. Input comparison normalizes the recorded temporary
workspace path. Capture reports record natural exit, terminal restoration and
child/socket/browser cleanup. They do not measure animation cadence.

## Reproduction and limits

Copy `rebuild.py`, `measure.py`, `compare.py`,
`acceptance-before-implementation.json` and
`acceptance-delete-before-measurement.json` into one scratch directory. From the
repository root, run `python3 SCRATCH/rebuild.py`, then
`python3 SCRATCH/measure.py before candidate`, then `python3 SCRATCH/compare.py`.
Rebuild temporarily restores the four predecessor production files and restores
current bytes in `finally`. Finish all builds before measuring; do not overlap
measurements with tests, builds or browser captures. `red.py` reproduces the
failing predecessor mutation and restores the four current production files in
`finally`. The recorded final comparison exits nonzero for the two short-draft
latency failures. The measurement helper accepts `UNDO_JOURNAL_SCENARIOS` to
select a comma-separated subset; the default runs all four workloads.

```sh
python3 docs/evidence/tui-rewrite/undo-journal/checks.py
node docs/evidence/tui-rewrite/undo-journal/capture-grouped.mjs SCRATCH/before-probe .omo/evidence/tui-rewrite/undo-journal/reproduced/grouped/before
node docs/evidence/tui-rewrite/undo-journal/capture-grouped.mjs SCRATCH/candidate-probe .omo/evidence/tui-rewrite/undo-journal/reproduced/grouped/candidate
python3 docs/evidence/tui-rewrite/undo-journal/compare-grouped.py .omo/evidence/tui-rewrite/undo-journal/reproduced/grouped SCRATCH/browser-grouped-comparison.json
node docs/evidence/tui-rewrite/undo-journal/capture-composer.mjs SCRATCH/before-probe .omo/evidence/tui-rewrite/undo-journal/reproduced/composer/before
node docs/evidence/tui-rewrite/undo-journal/capture-composer.mjs SCRATCH/candidate-probe .omo/evidence/tui-rewrite/undo-journal/reproduced/composer/candidate
python3 docs/evidence/tui-rewrite/undo-journal/compare-browser.py .omo/evidence/tui-rewrite/undo-journal/reproduced/composer SCRATCH/browser-composer-comparison.json
```

This removes repeated full-prompt retention during ordinary edits, preserving
undo depth. Whole-prompt replacements still retain the changed content, and
actual history changes can retain copies of history entries. The AppState text
mirror, its bounded fallback undo history and other legacy state remain. The
full rewrite, source-reduction target, sustained-runtime CPU target, earlier
typing latency failures, startup cadence, other platforms and final whole review
remain open. No backend contract or shortcut policy changes.

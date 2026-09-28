# Composer projection

The predecessor is `16ab0230547d73b0b9468eab683c880b920cda47`. Runtime composer
painting and pointer geometry now borrow the editor's atom buffer and project
only the serialized draft, badge labels and ghost suffix they consume. This
replaces the full `ComposerViewModel` construction, cloned editor model, reflow
and legacy mirror adapter in all four runtime presentation calls. The resolved
body and fixed chrome list are borrowed too. Row sizing measures borrowed text
width directly, removing a String, per-line strings and span vector for each
grapheme. The formula matches the locked Ratatui 0.1.2 implementation and the
existing chrome width helper, including empty lines and CRLF. No cache or
dependency is added.

Public owned view models and editor models remain available, with unchanged
reflow validation and error precedence. Their implementation is still retained;
this does not claim to finish the text/state rewrite. Public presentation and
runtime painting share the same row/chrome policy and surface tone.

## Preserved behavior

Atom widths determine the row budget. A separate string layout determines painted
cells, cursor placement and pointer coordinates. These calculations are deliberately
kept separate: an attachment atom has zero display width even though its serialized
placeholder occupies visible cells. If the rendered text differs from the editor's
serialized text, the existing atom parser derives the row budget from that mirror,
including its split-on-newline behavior for CRLF. Empty, unfocused Live and Plan
presentations collapse; focus and overlay-suppressed cursor visibility remain
separate inputs. Collapsed painting still uses the raw body in a single line.

Badge labels preserve attachment order and the completion states that display
`0 suggestions`. Ghost eligibility still follows rendered prompt text, while the
displayed suffix follows the atom editor. R20 records the existing repeated-text
defect when those public fields diverge. This change preserves that behavior.

Runtime no longer clones atoms to validate them through `from_atoms`: AppState's
slice is private, its public attachment path validates before installation, and
text, completion and undo operations preserve internally generated atom IDs.
The public owned model's `reflow` still rejects duplicate IDs. There is no new
public constructor, mutable editor access or relaxed public validation.

## Behavioral evidence

Existing public checks now paint loading/empty/ready completion badges, attachment
labels and narrow attachment geometry. An existing ghost-rendering check covers a
divergent prompt that is still a prediction prefix. No new test function is added.
The source-text delegation assertion is removed; it could not detect a geometry
regression. Public model, chrome, mouse-selection and full-frame checks remain.

All 13 extended public checks pass on the predecessor. A deliberate mutation that
budgets attachment rows from the serialized placeholder makes the narrow rendering
check fail: 12 checks pass and one fails. Restoring the candidate makes all 13 pass.
`red.py` restores the affected predecessor source before applying its mutation and
restores current source in `finally`. The initial mutation and corrected runnable
reproduction are both retained. Clippy's initial inclusive-range finding is also
retained with the corrected quality evidence.

## Measurement protocol

The retained predecessor binary is the journal candidate from the preceding
commit. Its source, fixture and executable hashes are checked before reuse.
`before-reuse-origin.json` preserves its original build receipt. The current
receipt identifies the reused executable with this step's predecessor commit
and lists hashes of all restored source files; it does not claim a new build.
Sixteen baseline runs establish limits before production changes. Each of the
four workloads has three timing runs and a separate glibc `memusage` run.
The inputs, 500 measured frames, ten warmups and 160×48 geometry are unchanged.
The middle timing run reverses binary order in the final comparison. Builds,
tests and any preceding browser captures finish before measurement. Final
candidate browser captures run after measurement.

Long typing must reduce allocation bytes, malloc calls and CPU by at least 10%.
Short typing cannot increase allocation bytes or malloc calls. All older stricter
limits remain; some bounds tighten against this baseline. Grouped-deletion CPU
must return to the pre-journal 0.40 ms/frame. These are public input, preparation,
render, diff and ANSI-encoding timings, excluding PTY and terminal-emulator latency.
Linux process ticks give CPU a resolution of 0.02 ms/frame in this workload.

## Results

The first candidate passed 24 of 28 limits. Long-draft allocations, malloc calls
and CPU missed the required 10% reduction, and short-draft p99 exceeded its bound.
That prompted the borrowed width correction. The complete first comparison and
its source patch remain in the performance archive under `phase1`.

The corrected candidate passes 25 of 28 frozen limits. Short-draft p95 is
168 µs against 133.1, p99 is 200 µs against 179.3, and CPU is 0.12 ms/frame
against 0.11. The paired predecessor also exceeds both short latency bounds,
at 161 and 189 µs, but has lower CPU at 0.10 ms/frame. The candidate is slower
in this comparison; these failures remain open. Raw tails include early bursts
and a late burst in candidate sample two. Their cause is unverified. No warmup,
input, CPU affinity or acceptance limit changed, and no unchanged rerun was
used to obtain a pass. Excluding the first 100 actions for diagnosis only still
leaves per-run candidate medians of 114, 111 and 118 µs against 110, 109 and
108 µs. The failure cannot be dismissed as warmup alone.

| Workload | p50/p95/p99 µs, before → candidate | CPU ms/frame | Allocation bytes | Malloc calls |
|---|---|---|---|---|
| Long typing | 311/328/348 → 216/235/352 | 0.32 → 0.22 | 329,864,607 → 73,547,952 | 4,645,584 → 106,974 |
| Short typing | 110/161/189 → 116/168/200 | 0.10 → 0.12 | 9,563,519 → 9,032,326 | 106,279 → 93,770 |
| Deep undo/redo | 329/347/364 → 233/243/264 | 0.34 → 0.22 | 351,933,761 → 95,629,298 | 4,947,751 → 409,164 |
| Grouped deletion | 424/550/578 → 292/368/390 | 0.44 → 0.30 | 480,600,088 → 137,376,571 | 6,680,914 → 633,627 |

Long typing cuts allocation bytes by 77.7%, malloc calls by 97.7% and CPU by
31.3%. Its peak heap falls from 2,250,203 to 2,193,328 bytes and RSS from 9,928
to 9,808 KiB. Short, undo and deletion memory measures also stay within bounds.
All 16 final pairs preserve scenario, frame count, history, ANSI byte count and
visible/oldest text. The archive contains all 80 raw reports: 16 baseline,
32 first-candidate and 32 corrected-candidate runs.

All 1,635 TUI checks and seven gated PTY checks pass, as do workspace check,
Clippy, formatting and suite gates. The workspace test suite was not rerun;
earlier predecessor CLI fixture failures remain recorded. Production source
has no net line change. Removing the structural test takes all TUI `src` Rust
files from 166,248 to 166,232 lines, still above the 90,941 target. Two existing
integration tests grow by 27 lines. The retained 1,356-line `layout.rs` changes
only its width helper and imports; its engine still needs replacement. The
other modified production files are at most 337 lines.

## Terminal evidence

The 27-frame composer journey extends the preceding 23-frame journey with a
Unicode multiline draft, 24×12 and 20×6 resizes, and a return to 140×40. Returning
to the original size must restore the draft's exact cells and cursor. The separate
11-frame journey retains deep undo, redo branching, and single-step restoration
of 80 grouped backspaces. Its predecessor capture is reused from the journal
candidate; the executable hash matches this step's reference.

All 38 paired frames match exactly in PNG bytes, cells, styles and cursor.
The narrow, compact and restored-size images were also inspected visually.
Both journeys use the same xterm.js, font, theme, geometry, inputs and reduced
motion settings on both binaries. Only observer callback counts are excluded
from terminal-state equality. Temporary workspace paths are normalized when
comparing input receipts. Reports check natural process exit, termios/protocol
restoration and child, socket, browser and temporary-directory cleanup. They do
not establish animation cadence or end-to-end input latency.

## Reproduction and limits

Copy `rebuild.py`, `measure.py`, `compare.py`, `checks.py` and
`acceptance-before-implementation.json` to one scratch directory. Use the source
and fixture versions recorded in the build receipts. From the repo
root, run `python3 SCRATCH/rebuild.py`, then
`python3 SCRATCH/measure.py before candidate`, then `python3 SCRATCH/compare.py`.
Rebuild temporarily restores predecessor sources and restores current bytes in
`finally`. Do not edit source during that operation or overlap measurement with
builds, tests or browser captures.

```sh
python3 SCRATCH/checks.py
node docs/evidence/tui-rewrite/composer-projection/capture-composer.mjs SCRATCH/before-probe .omo/evidence/tui-rewrite/composer-projection/reproduced/composer/before
node docs/evidence/tui-rewrite/composer-projection/capture-composer.mjs SCRATCH/candidate-probe .omo/evidence/tui-rewrite/composer-projection/reproduced/composer/candidate
python3 docs/evidence/tui-rewrite/composer-projection/compare-browser.py .omo/evidence/tui-rewrite/composer-projection/reproduced/composer SCRATCH/browser-comparison.json
node docs/evidence/tui-rewrite/composer-projection/capture-grouped.mjs SCRATCH/before-probe .omo/evidence/tui-rewrite/composer-projection/reproduced/grouped/before
node docs/evidence/tui-rewrite/composer-projection/capture-grouped.mjs SCRATCH/candidate-probe .omo/evidence/tui-rewrite/composer-projection/reproduced/grouped/candidate
python3 docs/evidence/tui-rewrite/composer-projection/compare-grouped.py .omo/evidence/tui-rewrite/composer-projection/reproduced/grouped SCRATCH/browser-grouped-comparison.json
```

Extract `performance.tar.gz` for all raw performance stages and build receipts,
`browser.tar.gz` for PNGs, full terminal states, ANSI streams and input/cleanup
reports, and `preparation.tar.gz` for the initial mutation. `files.json` hashes
every published artifact except itself. Runnable checks and reproduction
scripts are alongside the archives.

The AppState prompt mirror, fallback history, public owned models and separate
string viewport implementation remain. Source reduction, sustained-runtime CPU,
startup cadence, other platforms and the final whole-rewrite review remain open.
Earlier latency failures are not erased by a later passing comparison. No backend
contract, shortcut or appearance policy changes.

The independent reviewer approved this intermediate commit after checking all
55 pre-review artifact receipts, source hashes, 80 performance reports and 38
terminal pairs. `review.json` records the decision. It does not waive the three
failed short-draft limits or approve the whole rewrite.

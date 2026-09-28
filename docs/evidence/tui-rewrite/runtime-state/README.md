# Runtime-state projection replacement

The predecessor is `8c43e0319252c1afc6155c095eba742e0707eb98`. A compact,
pure runtime-state projection replaces the owned producer and its helper chain.
The private view borrows constant and existing text through `Cow<str>` and uses
static composer hints. `AppState::runtime_state` still returns the unchanged
public owned `RuntimeState`; kind-only and composer-disable consumers use the
borrowed view. Dynamic turn/tool summaries and permission enhancement still
allocate. There is no cache, dependency or backend change.

Shell, banner, replay failure, permission, non-tool cancellation, tool/activity
and ready precedence remain. Unknown banners still win. Tool summaries retain
effective identity. Queued/running details use transcript formatting, succeeded
details use truncated output, and failed details use output summaries. Nonempty
details retain whitespace, and digest sanitization stays at its existing points.
App enhancement keeps its raw activity checks separate from filtered activity
selection. Replay still independently disables the composer.

The old producer/helpers and five uncalled test-only functions are removed.
Production falls by 54 lines. Two existing behavioral tests gain 119 lines;
242 lines of unregistered test functions and fixtures are removed. No registered
test is removed. The TUI source tree totals 164,944 lines, above the 90,941-line
target. The new runtime module has 258 lines and its parent 419. Retained
lifecycle and chrome files still exceed 500 lines at 1,194 and 1,526.

## Behavioral evidence

The existing status journey now asserts complete public state values for ready,
sending, streaming, completion, cancellation, permission review/submission and
queued/running/successful/failed tools. The tool stage feeds provider progress
after scheduling so the turn is active; successful tools correctly have no
detail unless truncated output exists. A permission overlay check also asserts
that banner-derived degraded state disables input while the permission modal
owns the visible surface.

Both extended checks pass on the predecessor. Mutating its effective tool ID to
the invoked alias makes the status journey fail and leaves the overlay check
passing. Preparation logs retain mistaken fixture expectations, a transient test
compile error, and the first attempted mutation blocked by that compile error;
only the later completed mutation counts as red evidence.

A temporary oracle compares 143,640 complete runtime-state values with the
frozen predecessor producer. It combines shell/replay modes, banner precedence,
permission submission, cancellation scope, empty/whitespace/digest-bearing
details, activity status, tool status and effective identity. Every value matches.
The fixture and its registration are removed afterward. Initial oracle macro
compilation failure and the passing corrected check remain recorded.

All 1,635 TUI checks, including independent recorded cell/cursor/intent reference,
seven gated PTY checks, workspace check, Clippy, formatting and suite gates pass.
The workspace test suite was not rerun; earlier predecessor CLI fixture failures
remain recorded.

## Measurement protocol

A fresh predecessor release benchmark/probe was built from the clean pinned
source. Sixteen baseline runs preceded all source/test edits. Limits require
10% fewer long-typing malloc calls and 5% fewer allocated bytes, no allocation
increases elsewhere, and preserve all earlier stricter bounds.

Four workloads each use three timing runs and a separate memusage run: 500
frames, ten warmups, 160×48 geometry, no history. The middle paired sample
reverses executable order. No builds, tests, browser capture or profiling overlap
measurement. Timings include input handling, preparation, rendering, diffing and
ANSI encoding; they exclude PTY/emulator delivery.

## Results

The candidate passes 15 of 28 frozen limits. Long typing malloc calls fall 24.0%,
exceeding the 10% target. Allocated bytes fall 3.2%, missing the 5% target:
16,123,864 bytes exceed the 15,811,440.9 bound. Allocation bytes and malloc calls
fall in all four workloads. Peak heap and RSS bounds pass.

All twelve latency/CPU bounds fail. Both executables run much slower than the
pre-implementation baseline: long typing p95 rises from 236 µs in that baseline
to 633 µs for the paired predecessor and 595 µs for the candidate. The paired
long-typing CPU readings are 0.50 and 0.48 ms/frame, versus the earlier 0.22.
This does not establish a responsiveness improvement. The cause remains
unverified. A host diagnostic was captured afterward, while browser capture was
already active; it is not a baseline comparison or proof of a cause. No bound
changed and no unchanged performance rerun was used to seek a passing result.

| Workload | p50/p95/p99 µs, before → candidate | CPU ms/frame | Allocation bytes | Malloc calls |
|---|---|---|---|---|
| Long typing | 470/633/654 → 467/595/641 | 0.50 → 0.48 | 16,648,846 → 16,123,864 | 80,872 → 61,428 |
| Short typing | 243/315/329 → 248/308/326 | 0.26 → 0.26 | 7,942,401 → 7,449,681 | 74,840 → 56,926 |
| Deep undo/redo | 511/655/710 → 500/567/664 | 0.54 → 0.52 | 38,723,584 → 38,087,226 | 383,035 → 359,594 |
| Grouped deletion | 636/839/1032 → 639/946/994 | 0.64 → 0.66 | 52,777,985 → 52,256,403 | 604,193 → 584,763 |

All 16 paired output records retain exact workload, history, frame count, ANSI
byte count and visible/oldest text. All 48 raw reports remain available: sixteen
initial baseline records and 32 paired records, including failed measurements.

## Terminal evidence

Three actual-runtime PTY/xterm journeys cover 12 runtime frames, 31 composer
frames and 11 deep-undo/grouped-deletion frames. Runtime stages include sending,
streaming, queued/running/completed tools, permission review/submission/resolution,
completion, cancellation and failure. Both sides use fresh captures from their
recorded release probe. Final PNGs, cells, styles and cursors match exactly in
all 54 pairs, and all six processes restore terminal state and clean up their
process groups, sockets, browsers and temporary directories. Permission, completion, cancellation and failure screens were inspected visually.

The first runtime pair differed in two total elapsed labels: 5.6 vs 5.5 seconds
and 8.3 vs 8.2 seconds. The initial fixture used tiny event timestamps, leaving
those labels to wall-clock fallback. Its complete captures and mismatch report
remain in preparation evidence. The final fixture spaces recorded monotonic
events ten seconds apart, so the same projected durations dominate that fallback
on both sides. It also supplies the missing scheduler TaskCompleted event after
the tool result; otherwise the synthetic journey leaves a scheduled command
running after provider completion. Cancellation and failure use fresh provider requests with matching envelope
correlation IDs in the final journey. Independent review caught an intermediate
fixture that changed payload IDs but kept the first turn's correlation ID; it
misattributed cancellation to the first completed answer. The final script
asserts that cancellation follows its new prompt with its own 20-second duration,
the first answer remains above it, and failure follows its own prompt. The prior
misattributed frame fails the placement and duration assertions. An additional
assertion expecting the old completed footer to remain visible failed on the
predecessor: existing rendering shows only the latest assistant footer. That
mistaken assertion was replaced; its failed capture is retained alongside all
intermediate captures, scripts and attribution checks. These corrections change no production code.
No screenshot region or elapsed text is masked or normalized. Only observer
callback counts and isolated workspace paths are excluded from comparisons.

Reduced-motion captures establish state/layout parity for these journeys. They
do not establish animation cadence, live-provider behavior, or end-to-end latency.

## Reproduction

Copy `rebuild.py`, `measure.py`, `compare.py`, `checks.py` and
`acceptance-before-implementation.json` to scratch. Use the source/fixture
versions recorded in the receipts. From the repository root:

```sh
python3 SCRATCH/rebuild.py
python3 SCRATCH/measure.py before candidate
python3 SCRATCH/compare.py
python3 SCRATCH/checks.py
```

Rebuild temporarily restores the six predecessor production paths, including
removing the new runtime module, then restores current contents in `finally`.
Do not edit or run other builds during that operation. The recorded run used a
fresh clean predecessor build before edits, then `rebuild.py --candidate-only`.
The default command rebuilds both executables.

For the differential check, copy `verify-runtime.py`, `legacy-view_model.rs` and
`oracle-cases.rs`, then run `python3 SCRATCH/verify-runtime.py`. It compiles the
frozen producer in a temporary module, runs nextest and removes its registration
in `finally`. `red.py` runs on the predecessor production sources with the
extended tests applied: it mutates effective tool identity and restores source
in `finally`. The first green log and later completed red log are distinct from
preparation failures.

Browser captures use `capture-runtime.mjs`, `capture-composer.mjs` and
`capture-grouped.mjs` with arguments `SCRATCH/before-probe EVIDENCE_DIR` or
`SCRATCH/candidate-probe EVIDENCE_DIR`. Put each pair under `before` and
`candidate` below `.omo/evidence`. `browser.py` runs the three pairs and their
comparators. All earlier runtime fixture attempts are preserved in preparation evidence.

`performance.tar.gz` holds all raw performance reports and build metadata.
`browser.tar.gz` holds PNGs, complete cell states, ANSI streams, input sequences
and restoration/cleanup reports. `preparation.tar.gz` retains failed preparation
checks and the first runtime captures. `files.json` hashes every artifact except
itself. Frozen source stays outside compiled production code.

The whole rewrite remains unfinished. The remaining layout/state engines,
public owned composer models, duplicate prompt state and fallback history need
replacement. Source reduction, sustained resource behavior, startup/animation
cadence, the remaining feature/environment matrix and final independent review
remain open. The failed bounds are not waived by this intermediate commit.

The independent reviewer approved this intermediate commit after verifying all
63 pre-review artifact hashes, eight source receipts, the final patch, raw
performance calculations, 54 terminal pairs, six cleanup reports and corrected
request attribution. `review.json` records the decision and resolved fixture
findings. It does not waive the 13 failed limits or approve rewrite completion.

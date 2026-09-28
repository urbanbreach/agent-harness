# Frame, session and dock layout replacement

The predecessor is `f09849404888677de613a5df2646dd9c554a93ce`. The frame layout
now writes directly into `FrameLayoutPlan`. The intermediate `SessionShellLayout`,
duplicate replay hit-map assignment, repeated composer measurement, and unreachable
permission-rhythm branch are removed. Fixed vertical partitions use saturating
rectangle arithmetic instead of two constraint-solver/cache lookups. No cache,
dependency, backend or public-contract change is introduced.

The public plan retains zero-height `Some(Rect)` values. The footer wins when
root height is insufficient. Vertical splitting preserves raw horizontal fields,
including rectangles near `u16::MAX`. Startup centering remains distinct from
live insets. Replay's empty sidebar still reserves width without wheel targets;
child replay permissions still suppress the child footer without opening a live
permission dock. Todo reserves space before terminal sizing, and the details
overlay covers the post-todo body, including the terminal. Overlay priority and
model-notice transcript/hit-area shrinkage are unchanged.

Production layout code falls by 262 lines; tests and test-only helpers fall by
204 net lines. The source tree totals 164,478 lines, above the 90,941-line target.
The replaced layout file is 399 lines, its new session projection 296, and the
extended surfaces module 204. Moved layout tests occupy 252 lines. The untouched
permission geometry module still exceeds 500 lines.

## Behavioral evidence

Two existing journeys gain checks for todo/terminal/details wheel routing and
clicking a painted question option after compact resizing. Both pass on the
predecessor. Moving the details overlay below the terminal split deliberately
breaks the wheel test; the question test still passes. The first fixture assertion
incorrectly expected the overlay to start strictly after the todo's first row.
The existing reserve can overlap that row; the corrected assertion preserves
its measured reserve and tests routing. This slice does not change that geometry.

The initial temporary oracle compares 51,840 complete frame plans on both the
predecessor and candidate. Review added help and seven rewind states, bringing
the final comparison to 75,168 plans: 29 states, three themes (including custom
geometry), and 864 rectangles spanning breakpoints, tiny sizes, translations and
raw saturated coordinates. Every field, including hit maps, matches the frozen
producer. The temporary registration is removed; the legacy source and oracle
remain only as evidence. Initial timeout and moved-test compile failures are
retained as preparation evidence.

One registered test is removed because it exercised only a test-only lifecycle
helper, which is also removed. Uncalled dock assertion and event-fixture helpers
are deleted. Existing useful layout assertions move intact; one reference to a
deleted spacer constant becomes its unchanged value. All 1,634 TUI checks, seven
gated PTY checks, workspace check, Clippy, formatting and suite gates pass. The
full workspace test suite was not rerun; earlier CLI fixture failures remain
recorded.

## Measurement protocol

The baseline reuses the preceding slice's release benchmark and runtime probe.
Binary, fixture and production-source hashes were checked against the clean
pinned predecessor before reuse. The original build receipt is retained. Its
rebound `compiled_base` metadata was corrected before freezing acceptance; the
nested original receipt was unchanged.

Sixteen fresh baseline runs preceded implementation. Limits require 10% fewer
long-typing malloc calls and 5% fewer allocated bytes, no allocation increases
elsewhere, and preserve every earlier stricter bound. Earlier diagnostic malloc
stacks locate repeated layout/rhythm work; they are not acceptance measurements.

Four workloads each use three timing runs and one memusage run: 500 frames,
ten warmups, 160×48, zero history. Paired executable order reverses for the middle
sample. No build, test, browser or profiling work overlaps the measurements.
Timings include input handling, preparation, rendering, diffing and ANSI encoding;
they exclude PTY and emulator delivery. All 48 raw reports are retained.

## Results

The candidate passes 15 of 28 frozen limits. Long typing malloc calls fall
13.1%, meeting the 10% target. Allocated bytes fall 1.6%, missing the 5% target:
15,862,366 bytes exceed the 15,312,305.2 limit. Allocation bytes, malloc calls and
peak heap fall in all four paired workloads. RSS varies slightly in both
directions and stays within its limits.

Paired long-typing CPU falls from 0.48 to 0.42 ms/frame, with lower latency
quantiles. Undo/redo and grouped deletion also improve in these samples; short
CPU is unchanged. All twelve inherited absolute latency/CPU limits still fail.
The earlier baseline/host slowdown remains unexplained. These paired results do
not establish sustained responsiveness or end-to-end terminal latency. No bound
was relaxed and no unchanged performance rerun sought a passing result.

| Workload | p50/p95/p99 µs, before → candidate | CPU ms/frame | Allocation bytes | Malloc calls |
|---|---|---|---|---|
| Long typing | 467/597/660 → 405/526/558 | 0.48 → 0.42 | 16,123,536 → 15,862,366 | 61,419 → 53,380 |
| Short typing | 240/322/346 → 240/313/333 | 0.24 → 0.24 | 7,457,073 → 7,225,907 | 56,937 → 50,179 |
| Deep undo/redo | 501/593/663 → 440/523/585 | 0.50 → 0.46 | 38,101,442 → 37,834,744 | 359,619 → 351,558 |
| Grouped deletion | 633/857/1029 → 555/804/892 | 0.64 → 0.58 | 52,256,243 → 51,932,421 | 584,766 → 576,248 |

All 16 paired output records preserve workload, history, frame count, ANSI byte
count and visible/oldest text exactly.

## Terminal evidence

Four actual-runtime PTY/xterm journeys capture 12 runtime, 31 composer,
11 deep-undo/grouped-deletion and 16 layout frames. The layout journey adds
question resizing down to 20×8, restored draft/selection, terminal/todo/details
panes, and help at multiple sizes. All 70 final PNG/cell/style/cursor pairs
match exactly; only observer render/parse counts differ. All eight final runs
restore terminal state and clean up process groups, sockets, browser resources
and temporary directories. Compact question, overlapping panes and help frames
were inspected visually. These are reduced-motion captures; animation cadence and the full
feature/environment matrix remain open work.

The first layout attempt searched for a command absent from the exposed palette.
Its timeout, recording and forced cleanup remain in preparation evidence. The
fixture now uses Tab then 4 to open the terminal, and Tab leaves todo focus before
Ctrl+G opens details. Production code and performance samples are unchanged.

## Reproduction and review

`base.json`, `before-reuse-origin.json`, `sources.json`, `source.patch.gz` and
`environment.json` pin source and executable provenance. `legacy.tar.gz` preserves
the old producer; `oracle.py` temporarily registers it and always restores the
crate root. `checks.py` runs the documented quality and PTY commands. Build both
revisions with the release command in `build.py`; the saved benchmark paths live
in `before/binaries.json` and `candidate/binaries.json` inside `performance.tar.gz`.
`measure.py before candidate`, then `compare.py`, reproduce the four workloads
and enforce the frozen bounds. `browser.py` runs the four capture/comparison
scripts against the saved runtime probes. Run scripts from the repository root,
with their evidence files unpacked beside them and executable paths rebound to
the new build locations. `validation.json` records the first validation sequence; `layout-final-runs.json`
records the corrected layout capture pair.

`preparation.tar.gz` retains failed preparation checks and the initial smaller
oracle. `performance.tar.gz` contains initial and paired raw reports and logs;
`browser.tar.gz` contains terminal recordings, screenshots, cells and cleanup
reports. The manifest hashes every delivered artifact except itself.

Independent review approved the source direction and implementation and requested
the extra review/rewind oracle states. Review also corrected reproduction scripts to preserve the original build base
and include staged source changes in the patch. Earlier script versions remain
in preparation evidence. Final evidence review is recorded separately.
The full rewrite, original visual matrix, sustained resource targets and unverified
environments remain incomplete. No push is performed.

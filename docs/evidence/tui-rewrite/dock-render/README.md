# Dock projection and disclosure replacement

The predecessor is `ec2fa068a7658c31ef033540918c4d525366ac94`. The dock now reads
application state directly instead of building an owned intermediate projection. The private
`ControlDockViewModel`, input enum, variant enum, duplicate canvas fill and unused
shortcut strings are removed. Composer rendering takes four small values instead
of an owned dock model. Disclosure selection keeps the existing weighted priority
and first-tie order, then moves the selected spans instead of cloning combinations.
Public runtime context methods retain their labels, sanitization and cache priority.
Replay remains read-only. No backend, dependency, cache or public contract changes.

The source tree loses 684 lines overall, including test changes, and totals
163,794 lines. The 1,032-line disclosure module becomes three files of 291, 166
and 107 lines. Retained chrome and application projection files still exceed 500
lines. This completes the dock slice, not the whole TUI rewrite.

## Behavior and validation

An existing rendered-disclosure test now covers compaction summaries and disabled
connection hints at five widths. It passes on the predecessor; reversing the
summary/hint weights makes it fail. Existing context tests now use public methods.
The cache test verifies cache metadata wins over a pending model variant change.
The purity test compares rendered buffers instead of the removed private model.
One trivial canvas getter test is removed.

The controlled oracle compares 12,084 full styled buffers, cursors and public
context strings across 53 states, four themes and 57 dimensions. Every record
matches exactly. Review added explicit active multiline mode, remapped interject
and replace keys, starting-session hints, clear confirmation and background tasks.
Other cases include tiny widths, custom theme geometry, Unicode labels, todo focus,
replay failures, disconnected states, questions, overlays and rewind phases.
Temporary registration is removed; frozen predecessor source lives only in evidence.

The first comparison exposed wall-clock timer differences in 429 frames. The
fixture now supplies identical timestamps and normalizes timer origins on both
builds. The original differences and captures remain in preparation evidence.
Earlier compaction and model-variant fixture mistakes, including the single failed
TUI run before the corrected cache fixture, are also retained. Production code was
not changed to make those fixtures pass.

R22 in `docs/tui-rewrite.md` records the retained disclosure width defect. Candidate
fit uses scalar counts, while painting uses terminal cells. Foreground hints also
retain the predecessor's full-width fit followed by painting two cells narrower.
These rules preserve existing output; this slice does not claim to correct them.

All 1,633 TUI tests and seven gated PTY tests pass. Workspace compilation,
all-target/all-feature Clippy, formatting and suite gates pass. The full workspace
run executes 1,914 tests: 1,909 pass, five fail and eight are skipped. The failures
are four stale model assertions from two tests registered twice, plus the mock
launcher's old terminal-error assertion. All five fail again on the hash-checked predecessor under the same isolated
configuration. Their logs and exact name comparison remain in the evidence.

## Release measurements

The baseline executable and runtime probe were reused only after checking binary,
fixture and production-source hashes against the pinned predecessor. A separate
symbolized release diagnostic sampled every 200th malloc call. Forty of 267 samples
passed through dock construction, including 15 from unused shortcut strings.
That diagnostic includes setup and warmup and is not an acceptance timing run.

Sixteen baseline runs preceded implementation. Frozen limits require at least
10% fewer long-typing malloc calls and 5% fewer allocated bytes, no allocation
increases in the other workloads, and retain all stricter inherited bounds.

The final comparison alternates predecessor and candidate executables, reversing
order for the middle timing sample. Each workload has three timing samples and
one memusage run, with ten warmups and 500 measured frames at 160×48 and zero
history. No build, test, browser or profiler overlaps the measurements. Timings
cover input, preparation, rendering, diffing and ANSI encoding, excluding PTY and
emulator delivery. Allocation totals include process setup and warmup.

| Workload | p50/p95/p99 µs, before → candidate | CPU ms/frame | Allocated bytes | Malloc calls |
|---|---|---|---|---|
| Long typing | 185/201/267 → 182/205/293 | 0.18 → 0.18 | 15,862,358 → 15,034,861 | 53,380 → 40,581 |
| Short typing | 109/126/185 → 104/117/156 | 0.10 → 0.10 | 7,225,899 → 6,824,386 | 50,179 → 37,637 |
| Deep undo/redo | 198/215/231 → 196/216/232 | 0.20 → 0.20 | 37,830,496 → 37,002,999 | 351,557 → 338,758 |
| Grouped deletion | 253/310/342 → 249/294/313 | 0.26 → 0.26 | 51,932,413 → 50,964,802 | 576,248 → 563,449 |

The candidate passes 26 of 28 frozen limits. Long-typing malloc calls fall 24.0%
and allocated bytes fall 5.2%, meeting both dock targets. Allocation totals and
malloc calls fall in every workload. CPU is unchanged at the process-counter
resolution. Peak heap rises by 2,115 bytes in each paired workload; RSS moves
slightly in both directions. Both remain within their limits.

Two p99 limits fail: long typing is 293 µs against 272.8 µs, and short typing is
156 µs against 139.7 µs. Long-typing p99 is also 9.7% worse than the paired
predecessor. These samples establish an allocation improvement, not a latency or
resident-memory improvement. Both executables run much faster than the earlier
frozen baseline on this host; that shift is unexplained and is not attributed to
the dock. No limit was relaxed and no unchanged measurement was rerun for a pass.
All 16 paired workload/output records match, including frame and ANSI byte counts.

## Terminal evidence

Five actual-runtime PTY/xterm journeys capture 12 runtime, 31 composer,
11 undo/deletion, 16 layout and 13 dock frames. All 83 final PNG/cell/style/cursor
pairs match exactly, apart from observer render/parse counters. All ten final runs
exit naturally, restore termios and terminal protocols, and clean up process groups,
sockets, browser resources and temporary directories. The dock journey covers
compaction summaries, widths 100/80/60/40, clear confirmation, the cleared draft's
history suggestion, and disconnected hints. The confirmation, cleared suggestion,
compact disconnected state and wrapped composer were inspected visually.

The initial dock captures let the 800 ms confirmation expire before saving it.
The corrected fixture captures that transient frame without the extra stable-frame
wait, then rearms confirmation and sends Escape again after 100 ms. A first assertion
mistook the cleared draft's italic history suggestion for entered text. The final
check requires the cursor at the start of that suggestion and italic cells.
Original captures, timing attempts and failed assertions remain in preparation
evidence. Production code and release samples did not change.

These reduced-motion captures do not establish animation cadence or end-to-end
latency. The original whole-rewrite feature/environment matrix remains unfinished.

## Reproduction

Run scripts from the repository root. Unpack archives beside the scripts and
rebind saved executable paths when reproducing elsewhere. `base.json`,
`before-reuse-origin.json`, `sources.json`, `source.patch.gz` and `environment.json`
pin source and binary provenance. `build.py` records the release build command.
`measure.py before candidate` runs the paired workloads; `compare.py` enforces
all frozen limits and intentionally exits unsuccessfully for the two p99 misses.
`performance.tar.gz` retains the initial 16 and final 32 raw runs.

`reference.py` temporarily installs hash-checked predecessor files and runs the
public behavioral checks plus `oracle.py before`. It always restores the candidate
files. `oracle.py candidate` records and compares every frame through EOF, restoring
its temporary registration afterward. `reference.py --red` reproduces the scoring mutation against those same frozen
production files and the extended behavioral test.
`checks.py` runs quality and gated PTY checks. `workspace.py` runs the full workspace
suite with an empty temporary `XDG_CONFIG_HOME`, leaving `HOME` unchanged, to avoid
the previously documented invalid global provider configuration. No user settings
are read into evidence or changed.

`browser.py` runs the five actual-runtime PTY/xterm journeys. `browser.tar.gz`
contains recordings, screenshots, complete cell data and cleanup reports.
`checks.tar.gz` retains logs; `preparation.tar.gz` retains failed preparation and
initial oracle evidence. `profile.tar.gz` holds diagnostic stacks and commands.
`files.json` hashes every delivered artifact except itself.

To reproduce the diagnostic separately, build the predecessor with `cargo rustc
--release -p harness-tui --test rewrite_performance_test --message-format=json --
-C strip=none`, saving stdout as `profile-build.jsonl`, then run `profile.py`.
The delivered runner uses its own directory; original absolute-path scripts remain
in preparation evidence. This diagnostic is not part of the paired acceptance run.

Independent final review approved this bounded dock commit with no blocking
findings. It checked all source/artifact receipts, the source patch, oracle and
terminal comparisons, representative screenshots, raw performance calculations,
and matching predecessor failures. `review.json` records the decision. Approval
does not waive the two p99 misses or declare the whole rewrite complete.

# Durable settlement and prepared-prefix reuse

Plain durable turns now rebuild the last canonical turn and its appended suffix,
keeping the earlier presentation in place. The existing converter handles both
full and suffix conversion; no second state engine or persistent index was added.
Canonical validation, event storage, generation changes and coordinator authority
are unchanged. No backend source changed in this slice.

The guard proves request, provider and task ownership across the boundary before
applying a batch. Tools, permissions, live assistant fragments, rewinds, lineage,
ambiguous identifiers, trimming and terminal-task resurrection use full conversion.
A reopened completed task can restore the previously sixth-oldest task row; the
retained display map cannot recover it alone. Prepared layouts are reused only
for terminal, tool-free, permission-free prefix turns. Other rows can depend on
global queued or task state. Existing footer repair still runs across all turns.

This is a bounded optimization during the rewrite. Eligibility and some remaining
presentation helpers still scan history. The legacy state engine, complex-turn
converter and many formatters remain. Production source grows by 278 lines and
unit-test source by 42 lines. Whole TUI source is 167,934 lines
across 564 files, 7.67% below the original. The 50% target
and removal of the original state engine are not achieved. The suffix guard is
198 lines; the shared converter is 388. Existing tests were extended and moved
into smaller files; the maintained test count is unchanged.

## Behavior and review

All 1,666 deterministic TUI tests pass with seven configured skips. The final
source matches all 543 main records, eight plan records, four wrapping records
and 733 exact-clock chat/tool frames, including ANSI, cells, styles and recorded
interaction outcomes. Six paired xterm screenshots match byte for byte. They
replay controlled renderer output; they do not measure real-runtime cadence.
Terminal snapshots match apart from asynchronous render callbacks: candidate
`renderCount` is one or two higher; `parsedCount` matches. The exact counts are
recorded in `browser/comparison.json`.

The extended existing settlement journey checks nested older tool/artifact
updates, pending-question ownership, task retention, invalid durable histories,
and warm versus cold painted buffers and interaction snapshots at each event.
It includes a newly buffered turn after two completed turns. Removing the task
reopening guard produces a real red result: the sixth-oldest row disappears.
Restoring the guard makes the unchanged assertion pass. The same complete journey
passes against the pinned original.

Conditional selection hashing exposed two implicit invalidation dependencies.
The queued-prompt reference journey caught stale local echoes. The extended
existing `/new` journey caught old-session text after a reset; it fails before
explicit reset invalidation, passes afterward, and passes on the original.
Both navigation reset paths now invalidate, as does every nonempty local echo.
Independent review checked all production activity mutation/reset callers.
These were candidate regressions, not intentional behavior changes.

Unconditional echo invalidation also exposed a separate original defect: eight
file/subagent-mention submission frames acquire their missing first local prompt.
Those eight original frames were independently checked against the frozen oracle.
This slice preserves their first-empty-view timing (R15 in `docs/tui-rewrite.md`)
and changes no golden snapshot. The unconditional-invalidation captures remain
in `diagnostics/tui-settled-regression-comparison` for an intentional later fix.

Seven gated P0-03/P1-04 checks pass. All six P1-04 terminal sessions exit and
close their PTYs, covering three sizes, Unicode/ASCII, following, detached
viewports, resize bursts and reduced motion. The final release probe also runs
the real PTY/xterm rewind and click-burst workflow, and all four restoration
probes pass. With `/dev/full`, only termios restoration is observable because
protocol output cannot reach the terminal. Fixtures are synthetic and offline.

Scoped all-target/all-feature Clippy, workspace check, formatting and suite gates
pass. Source approval and the final evidence review are recorded separately.

## Measurements

The new workload starts with 1,000 turns and 6,000 durable events. Each update
adds six events and three canonical settlements, then prepares and renders one
160×48 frame. Ten warmups precede 200 measured updates. All updates are parsed
before timing. Untimed assertions require every update, the expected projection
generation, no canonical error, the newest answer and the oldest retained prompt.

Acceptance was frozen before implementation at 2026-09-27 16:45:45 UTC: at least
30% lower settlement p95, p99, CPU and allocation totals, with RSS within 10%.
The original fourteen renderer/resource limits remain unchanged. Later fixture
edits add only post-measurement assertions; the frozen limits were not relaxed.
The shared nextest slow-timeout override now includes this benchmark so reference
and allocation runs can finish. The same runner timeout applies to all builds.

The final set contains 63 serial timing runs and 21 separate allocation runs,
interleaving original (`1bb0f989`), preceding (`d7df6d0f`) and candidate executables.
Order reverses on the second repetition. Each timing value below is the median
of three runs. Whole-process allocation and peak-heap totals come from one
separate `memusage --no-timer` run per build/workload, including construction.
Builds and terminal captures finish before measurement. Source, fixture and
executable hashes identify the inputs.

| Settlement metric | Original | Preceding | Candidate |
| --- | ---: | ---: | ---: |
| p50, µs | 72,311 | 65,996 | 1,213 |
| p95, µs | 79,751 | 71,797 | 1,305 |
| p99, µs | 80,571 | 72,804 | 1,389 |
| CPU, ms/frame | 72.25 | 65.85 | 1.2 |
| RSS, KiB | 80,764 | 50,580 | 47,696 |
| Allocated bytes | 9,197,177,685 | 37,900,677,305 | 347,790,939 |
| Peak heap, bytes | 55,946,826 | 35,051,206 | 35,053,311 |

Settlement CPU falls 98.18% and allocations fall
99.08% versus the preceding implementation. RSS changes
-5.70%; peak heap is essentially unchanged. This removes repeated
work without demonstrating a substantial reduction in retained heap.

Stream p99 is 996 µs versus 985 µs; resize p99 is
2,727 versus 2,743 µs. All nineteen unchanged acceptance checks pass.
Terminal byte counts, final/oldest text and workload counts match the preceding
build in all 28 samples and match the original in all four settlement samples.
Static p99 values are 182, 130, 141 and
174 µs for startup, idle, typing and scrolling. Startup p99 was
172 µs in the preceding build; raw repetitions expose the variability.
Static CPU ticks are too coarse to claim a typing or idle CPU improvement.

The diagnostic build measured median canonical application at 2 µs, repeated
presentation conversion at 14,264.5 µs, and full frame preparation at 19,484.5 µs.
Suffix conversion alone reduced latency but still allocated 35.73 GB, failing
the frozen allocation target. Reusing the already-existing prepared prefix
removes the remaining repeated work. Diagnostic instrumentation and samples are
retained separately. The first full candidate's measurements remain under
`initial/`; it failed the queued-prompt parity check and is excluded from final
acceptance. Later echo/reset corrections are in the final measured source.

These timings cover public event/input handling, frame preparation, painting,
Ratatui diff and Crossterm encoding to a counting sink. They are not PTY or
browser-visible latency, sustained runtime CPU or a long-duration memory-growth
measurement. Fixed-viewport resize can invoke `tput`; its child CPU is outside
the process CPU sample. Existing sustained typing/burst CPU and startup-cadence
limitations remain open. Other platforms and real providers remain unverified.

## Reproduction

Use nextest. Build and retain all three versions before running the serial
measurement script. `performance/measure.py` records local paths and commands;
adapt paths when reproducing elsewhere. `performance/compare.py` checks the
frozen limits and output equality. Raw samples, order, metadata, receipts and
logs are compressed beside them. The initial candidate's retained executable
is `/tmp/tui-settled-initial/candidate.bin`; its receipt retains its former path.

```sh
cargo nextest run --profile ci -p harness-tui --all-features
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tui --all-features --ignore-default-filter -E 'binary(p0_03_pty_recorded) | binary(p1_04_pty_recorded)'
cargo nextest list --release --profile perf -p harness-tui --all-features --test rewrite_performance_test --list-type binaries-only --message-format json
cargo build --release -p harness-tui --all-features --example rewrite_probe --example resource_probe
node scripts/qa/measure-rewrite-latency.mjs RETAINED_REWRITE_PROBE .omo/evidence/settlement-workflow --workflow-only
python3 scripts/check-tui-restoration.py --binary RETAINED_RESOURCE_PROBE --output /tmp/settlement-restoration
```

`checks/commands.json` includes exact frame/motion commands and output locations.
Browser manifests retain the ANSI producer, source hashes, emulator and font.
The full rewrite still requires broader source replacement, supported-feature
signoff, sustained runtime/resource evidence and final independent approval.

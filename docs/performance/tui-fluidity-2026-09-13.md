# TUI fluidity measurements, 13 September 2026

This experiment compares the original Harness implementation at
`fd541a07eba60ef6556c095cd79c05c00fc808f6` with the modified builds identified in the evidence file.
It measures the terminal UI, including projection, layout, Unicode wrapping,
selection, syntax highlighting, terminal diffing, ANSI encoding, scheduling, and
output backpressure. Provider/network latency is outside the experiment.

## Measured results

The final build presents 227 frames/s while typing and **257 frames/s during
live-update bursts** through the real PTY. All 14 compared warm-render workloads
fit the 8.333 ms budget for 120 Hz at p95. These are separate processing and
runtime measurements, subject to the scope below.

The [machine-readable evidence](tui-fluidity-2026-09-13.json) contains every raw
frame sample, each independent run, CPU/RSS/peak-RSS readings, terminal byte counts,
intermediate results, executable/source hashes, and validation receipts. The
hashes identify the measured builds before the later Ponytail cleanup; historical
results do not exempt current code from performance gates.

Scenario suffixes denote retained history size. `code-500-lines` denotes 500
measured code-line appends after ten warm-up appends, with 100 prior activities.
The event-history comparison uses 30 measured updates on both versions; the
additional 120-update final run also passed the processing target (p95 7.245 ms).

| Scenario | p95 ms before → after | CPU ms/frame before → after | RSS MiB before → after |
|---|---:|---:|---:|
| startup-0 | 0.175 → 0.168 | 0.167 → 0.167 | 12.8 → 12.3 |
| static-100 | 0.150 → 0.143 | 0.083 → 0.083 | 16.7 → 14.9 |
| static-10000 | 0.624 → 0.373 | 0.583 → 0.333 | 351.0 → 219.6 |
| scroll-10000 | 1.418 → 0.601 | 1.333 → 0.583 | 351.1 → 220.0 |
| typing-10000 | 0.687 → 0.431 | 0.667 → 0.417 | 351.1 → 219.8 |
| stream-100 | 1.343 → 1.249 | 0.833 → 0.750 | 17.8 → 16.5 |
| stream-10000 | 16.632 → 6.285 | 15.750 → 5.667 | 367.7 → 247.6 |
| code-100 | 5.387 → 1.452 | 3.417 → 1.083 | 30.5 → 26.7 |
| tool-10000 | 14.692 → 5.086 | 14.417 → 4.750 | 368.0 → 247.0 |
| hover-10000 | 1.602 → 0.969 | 1.583 → 1.000 | 351.3 → 219.8 |
| selection-10000 | 53.258 → 0.837 | 29.917 → 0.750 | 448.1 → 232.7 |
| resize-10000 | 8.686 → 4.511 | 4.583 → 2.167 | 614.2 → 418.4 |
| stream-events-10000 | 636.372 → 6.531 | 601.333 → 6.333 | 376.8 → 256.4 |
| code-500-lines | 20.881 → 4.261 | 11.440 → 2.520 | 44.2 → 32.5 |


The largest reductions are event-history update latency (99.0%), selection
latency (98.4%), and 500-line code-stream latency (79.6%). Large-history
steady RSS falls from 351.0 to 219.6 MiB; selection falls from **448.1 to
232.7 MiB. Resize peak RSS falls from 614.2 to 418.4 MiB**. The 10,000-entry
stress corpus exceeds the normal text-eviction limit by design.

Cold first-frame time for 10,000 settled entries improves from 511 to 262 ms;
it remains a one-time layout cost, not a 120 Hz frame. Terminal bytes fall by
95.5% during scrolling and 95.1% during resizing. Static and hover-only
workloads emit no changed terminal bytes in this fixture.

### Production runtime and resource use

| Runtime scenario | Frames/s before → after | CPU % before → after | RSS MiB before → after |
|---|---:|---:|---:|
| idle | 0.00 → 0.00 | 0.00 → 0.00 | 10.37 → 9.93 |
| startup | 7.00 → 6.50 | 0.75 → 0.50 | 10.34 → 9.82 |
| typing | 81.00 → 227.25 | 7.00 → 8.25 | 11.27 → 11.30 |
| burst | 944.75 → 257.00 | 24.00 → 9.25 | 10.66 → 9.44 |
| slow-burst | 55.50 → 59.25 | 5.75 → 5.00 | 10.05 → 9.37 |


The previous burst path repainted almost every incoming update. Coalescing keeps
current state visible while cutting burst CPU by 61.5% and emitted bytes by
74.7%. Typing presents 2.8× as many frames: total CPU rises from 7% to
8.25% of one core, while CPU per presented frame falls by about 58%. The
higher refresh rate has a measurable cost; idle still parks.

Default-cadence received frame intervals have p95 5.07 ms for typing and
5.82 ms for bursts. The slow-reader case intentionally cannot sustain 120 Hz;
it verifies bounded output under backpressure. Its final-run RSS grows by only
about 8 KiB during the measured interval. Startup's discrete decorative state
changes remain slow and do not cap input presentation.

### Refresh calibration

The existing environment control was also measured with three typing and three
burst runs at each setting:

| Cadence | Scenario | Frames/s | CPU % | p95 received interval ms |
|---|---|---:|---:|---:|
| 6 ms | typing | 171.50 | 8.25 | 7.35 |
| 6 ms | burst | 181.00 | 7.00 | 7.58 |
| 8 ms | typing | 137.75 | 8.00 | 9.11 |
| 8 ms | burst | 143.00 | 7.00 | 9.24 |

The 4 ms default leaves more scheduling headroom. At 6 ms, measured p95 received
intervals still fit the 120 Hz budget on this host. At 8 ms, average throughput
exceeds 120 frames/s but p95 intervals exceed 8.333 ms. Immediate interactions and
separate motion deadlines can add presentations, so the coalescing interval is
not a strict global FPS limit. Raw inputs and received timestamps vary with OS
scheduling; the JSON records input counts and individual runs.

### Frame-history retention

With telemetry disabled, the original runtime retained **100,000 completed frame
acknowledgements**. The isolated retention test grew from 6,012 to 34,292 KiB RSS
(27.6 MiB growth). The final code retains zero acknowledgements after the
same run and grows from 6,008 to 6,712 KiB (0.69 MiB growth). This fixes the
specific accumulating frame-history defect; it does not claim that process RSS
never changes.

## Source comparison

Grok Build was inspected at commit
`75810042ca2762aa0b0fa17864f3f68823ccbea5`:

- Its [display refresh setup](https://github.com/xai-org/grok-build/blob/75810042ca2762aa0b0fa17864f3f68823ccbea5/crates/codegen/xai-grok-pager/src/app/display_refresh_startup.rs)
  provides an adjustable draw interval and optional display probing.
- Its [presentation loop](https://github.com/xai-org/grok-build/blob/75810042ca2762aa0b0fa17864f3f68823ccbea5/crates/codegen/xai-grok-pager/src/app/event_loop.rs)
  coalesces presentation requests and limits concurrent output.
- Its [scrollback renderer](https://github.com/xai-org/grok-build/blob/75810042ca2762aa0b0fa17864f3f68823ccbea5/crates/codegen/xai-grok-pager/src/scrollback/render.rs)
  restricts work to the visible window.

Harness already had a bounded writer, pacing controls, Unicode support, and
incremental highlighting. The implementation reuses those facilities. No new
dependency, rendering framework, display probe, or source-code copy was needed.
Grok Build itself was not benchmarked; these are Harness before/after results.

## Changes

- Batch terminal cells in row order through Crossterm, preserving OSC 8 link
  boundaries and safety checks. Identical hyperlink metadata no longer forces
  linked text to be repainted.
- Retire completed frame acknowledgements even when telemetry is disabled.
- Use a 4 ms default input/live-update flush cadence, resize coalescing window,
  and continuous-animation cadence. Bound each live-update batch to 2 ms.
  Preserve the slower intended timing of discrete spinners and decorative effects.
- Reuse immutable transcript sections, selection source data, and highlighted
  lines through the existing bounded caches. Remove redundant copies and blank
  cell padding.
- Restrict rendering and hit testing to visible sections using binary search.
  Restrict each activity's durable-event scans to its sequence range.
- Use the installed Unicode segmentation library instead of the handwritten
  allocating splitter. Trim capped transcript text at complete grapheme boundaries
  and invalidate affected projections.
- Skip tab expansion when there are no tabs, retain preformatted spans when the
  line already fits, and skip hyperlink projection when there are no links.
  Existing wrapping/copy tests cover both tabs and spaces at narrow and wide sizes.

## Cadence audit

| Boundary | Result |
|---|---|
| Input and live updates | Shared 4 ms default flush; existing 1 to 100 ms environment control retained |
| Live-update processing | At most 16 updates or 2 ms per batch; input keeps scheduling priority |
| Resize coalescing | 4 ms window, independent of the slower gesture classifier |
| Continuous motion and fades | Configured fast cadence; wall-clock speed and long-session phase period preserved |
| Discrete indicators | Spinners, startup glyphs, and background indicators retain their intended slower steps |
| Writer | One frame in flight; bounded backpressure; acknowledgements retired with telemetry off |
| Idle | No recurring redraw when there is no visible motion or changed content |

The 80 ms wheel classifier and the standalone 16 ms whole-row drag-autoscroll
step determine gesture behavior and scroll speed; they are not presentation caps.

## Measurement method

Measurements use release optimization on an Intel Core i7-12800H, Linux x86_64,
Rust 1.98.0, with 62 GiB RAM. CPU frequency remains OS-controlled; the machine uses
the `powersave` governor with `performance` energy preference. Original and
modified executables are preserved separately. Workloads
run serially, with no concurrent build or test jobs during measurements. Each main
scenario has three independent processes, ten warm-up frames, and 120 measured
frames. Results report the median of the three per-process measurements.

The render benchmark includes state updates, production rendering, Ratatui
diffing, the production ANSI backend, and a synchronous output sink at 160×48
cells. Large-history cases retain 10,000 synthetic activities by disabling the
normal transcript text eviction limit inside the fixture only. This keeps
the stress corpus equal before and after; its RSS is not typical-session RAM.
The event-history case also supplies 10,000 durable events and uses 30 measured
frames for the matched comparison. The long-code case appends 500 function lines
inside an open Rust code fence, after ten warm-up updates.

CPU time comes from Linux process counters (10 ms resolution). RSS and peak RSS
come from `/proc`; allocator retention is included. Cold first-frame time is
recorded separately from warm-frame percentiles. Raw samples accompany the final
tables so outliers and measurement resolution remain visible.

The runtime benchmark uses the actual event loop and writer with a real 160×48
PTY, truecolor enabled, 1.5 seconds of warm-up, and four measured seconds per run.
It covers a parked replay, the startup surface, continuous typing, 1 ms live
updates, and a reader limited to approximately 5 KiB/s. CPU percentages refer to
one core. Output frame counts use synchronized-update end markers.

A PTY does not emulate terminal rendering or prove physical monitor refresh.
Multiple frame markers can arrive in one read. These measurements establish
Harness's production output throughput and processing headroom; terminal,
compositor, display configuration, and remote connection can limit visible Hz.
The isolated render benchmark excludes scheduler waits. The runtime benchmark
includes those waits but uses small offline sessions. Full CLI/provider state adds
work and memory; these results do not guarantee 120 Hz for arbitrary history,
code-block length, window size, or terminal. No physical display rate was available
from this session, and other operating systems were not performance-tested.

## Reproduction

```bash
python3 scripts/measure-tui-performance.py --output /tmp/harness-render-perf
python3 scripts/measure-tui-performance.py --scenario stream-events --frames 30 --output /tmp/harness-event-perf
python3 scripts/measure-tui-performance.py --scenario code --frames 500 --output /tmp/harness-code-perf
cargo build --release -p harness-tui --example resource_probe
python3 scripts/measure-tui-runtime.py --binary target/release/examples/resource_probe --output /tmp/harness-runtime-perf.json
HARNESS_PERF_ACK_FRAMES=100000 cargo nextest run --release --profile ci -p harness-tui --lib -E 'test(frame_acknowledgements_are_retired_without_telemetry)' --success-output immediate
```

`HARNESS_TUI_MIN_DRAW_MS` remains the existing calibration control (1 to 100 ms,
default 4). An 8 ms interval allows nominal 125 Hz, 6 ms allows 167 Hz, and 4 ms
allows 250 Hz before processing and output costs. Idle sessions park instead of
continuously drawing at those rates.

For a fresh original-code comparison, create a detached worktree at the base
commit above, copy only the benchmark fixture into it, and build into a separate
`CARGO_TARGET_DIR`. Capture `cargo nextest list --profile perf --release -p
harness-tui --lib --list-type binaries-only --message-format json` and `cargo
metadata --format-version 1`, then pass the saved paths through the driver's
`--binaries-metadata` and `--cargo-metadata` options. Preserve or copy the binaries
before rebuilding another checkout. The original extended-fixture worktree used
here differs only in the benchmark file and its opt-in timeout setting.

## Validation

- Workspace Nextest run: 4,611 passed, 14 skipped after excluding three
  configuration-example expectations described below.
- Release performance gates: 3 passed; the existing 10,000-entry resize/anchor
  contract measured p95 0.464 ms, below its tightened 8.333 ms gate. This
  TestBackend-only gate is distinct from the full ANSI resize workload above.
- 100,000-frame acknowledgement regression: passed.
- Final TUI Nextest run: 1,866 passed, 5 skipped.
- Native PTY checks: 7 passed, including production startup/input/resize/exit
  and emulator captures at 80×24, 120×40, and 160×50. All captured children exited
  and PTYs closed.
- Workspace Clippy with all targets/features and `-D warnings`: passed.
- Formatting, diff whitespace checks, Python compilation, and the canonical
  `quality-gates` lane: passed.
- After Ponytail cleanup: 1,866 TUI tests, TUI Clippy, quality gates, and all three
  release gates passed; scrolling/selection p95 was 0.588/0.824 ms (three 120-frame runs).

The unfiltered workspace run passed 4,611 tests and failed three existing tests
that assume the repository example configuration still contains the original
Umans provider/model entries. The working tree already had user edits to
`harness.jsonc`; those edits were preserved. The affected tests are
`root_runtime_example_uses_canonical_public_keys`,
`adding_anthropic_auth_provider_to_real_config_works`, and
`umans_provider_has_api_key_env_without_auth_provider`. Their code was unchanged.

## Remaining limits

The first improvements still left event-heavy histories and long code streams
outside the target. The additional event-range and wrapping changes brought every
covered warm workload inside it; the final measurements found no remaining miss
of the stated processing target. Existing caches remain bounded, output remains
bounded, and no dependency or replacement rendering architecture was added.

Cold layout and work on a growing active block still depend on content size.
Unbounded workloads, terminal emulation, physical presentation, full provider
sessions, and other hardware remain outside this evidence. Further changes to
those areas need measurements of the workload that warrants them; the supplied
runners make that comparison repeatable.

# Assistant-part assembly replacement

The 401-line assembler consumes prepared tool sections and builds fallback
content only when needed. It replaces the cloned tool map, eager fallback
text/reasoning copies, temporary concatenated-prefix Strings and manual
insertion indices. Stable sequence sorting preserves response order. Rendering
remains immutable; backend contracts and coordinator authority are unchanged.

The section module is now 341 lines. Its existing preparation, turn-header and
edit-coalescing logic remains; that retained code is not claimed as rewritten.
The redundant argument bundle is removed. Four existing tests move to a 179-line
file with formatting changes only. The net reduction is 351 production lines
and three test lines, including test-module declarations. Whole TUI source is
167,614 lines across 562 files, 7.85% below the original. The 50% source target
and replacement of the remaining state engine and formatters remain unfinished.

## Behavioral evidence

The existing tool-boundary check failed with the empty replacement assembler,
then passed with the implementation. Its expected output requires settled text
before a tool and streaming text afterward. The red result is
`checks/tui-assistant-parts-red-behavior.log.gz`; the earlier `red.log.gz` only
records a mistaken filter that selected no tests and is not behavioral evidence.
No maintained test was added.

All 1,666 deterministic TUI tests pass with seven configured skips. The final
source matches all 543 main reference records, eight plan records and four
wrapping records, including ANSI, cells, styles, cursor, inputs and intents.
All 733 exact-clock chat/tool ANSI and text pairs match the preceding captures.
This includes reasoning, tool families, disclosure, narrow terminals and motion.

Six xterm screenshots at 0, 330 and 660 ms are byte-identical. They replay the
preceding and initial replacement's ANSI through the same browser and font.
The final capacity correction produces identical ANSI for all 733 frames, so
those images also represent the final output. Producer metadata retains the
actual source that made the replay inputs. Terminal snapshots match except
asynchronous render/parser counters. These captures do not measure real-runtime
animation cadence or browser-visible latency.

Seven gated P0-03/P1-04 tests pass on the final source. The six P1-04 sessions
cover Unicode/ASCII and three sizes, including following, detached viewports,
resize bursts and reduced motion. Every child exits and PTY closes. The
initial replacement also passes the real PTY/xterm rewind/click-burst workflow,
delivering its live notice in 49.29 ms, and passes the four restoration probes.
The capacity-only correction followed those workflow/restoration captures.
With `/dev/full`, only termios restoration is verifiable because protocol bytes
cannot reach the terminal. All events in these fixtures are synthetic.

Scoped all-target/all-feature Clippy, workspace check, formatting and suite
gates pass. Independent source review found no semantic regression in event
filtering, fallback order, committed text-before-tools, legacy fragments, live
suffixes, reasoning timing/source IDs or error placement.

## Resource measurements

The final set has 54 serial timing runs and 18 separate allocation runs. Each
uses 1,000 complete historical turns, ten warmups and 200 measured frames, except
empty startup. Values are medians of three runs; allocation totals come from
one separate `memusage` run per build/workload. The runner interleaves original,
preceding and candidate binaries, reversing order on the second repeat.
Compilation and captures finished before measurement.

The original is `1bb0f989`; the preceding source is `f5bd55e5`; the candidate is
that commit plus `source.patch.gz`. The benchmark and shared journey are
unchanged. Source, fixture and executable hashes identify the measured inputs.

| Workload and metric | Original | Preceding | Candidate |
| --- | ---: | ---: | ---: |
| Stream p95 / p99, µs | 4,037 / 4,293 | 955 / 993 | 953 / 996 |
| Stream CPU, ms/frame | 2.20 | 0.65 | 0.65 |
| Stream RSS, KiB | 49,120 | 33,296 | 33,312 |
| Stream allocated bytes | 744,463,506 | 312,391,293 | 312,089,387 |
| Resize p95 / p99, µs | 5,012 / 5,129 | 2,656 / 2,766 | 2,640 / 2,751 |
| Resize CPU, ms/frame | 2.85 | 1.00 | 0.95 |
| Resize RSS, KiB | 63,304 | 39,640 | 39,796 |
| Resize allocated bytes | 670,823,764 | 348,833,584 | 348,560,918 |

Stream and resize costs are essentially unchanged from the preceding build.
Allocated bytes fall 0.10% and 0.08%; RSS rises 0.05% and 0.39%. Peak heap differs
by three bytes in both workloads. This slice simplifies assembly; it does not
demonstrate a substantial retained-memory or speed improvement.

Startup p95 rises from 169 to 248 µs and p99 from 259 to 267 µs. Its preceding
p99 was 168 µs in the initial run, so variability is visible even in retained
binaries. The cause is not established. Final static p99 values are 267, 132,
142 and 168 µs for startup, idle, typing and scrolling. All fourteen unchanged
frozen renderer/resource checks pass. Output bytes, oldest retained history,
visible content and workload counts match the preceding build in all 24 timing
and allocation samples. No content is dropped.

The first implementation used `unzip()` for the output vectors. It reserved
four slots for a one-part turn and increased retained heap by roughly 288 KiB
over 1,000 turns. Explicit capacities restore the preceding heap footprint.
The stdlib capacity diagnostic, initial source and all initial samples remain
under `diagnostics/` and `initial/`; those samples are excluded from final
acceptance. This was a candidate implementation regression, not a reference
defect. The retained initial executable is
`/tmp/tui-assistant-parts-initial/candidate.bin`; its measurement receipts keep
the original path and hash.

These measurements include public event/input handlers, preparation, painting,
Ratatui diffing and Crossterm encoding to a counting sink. They do not measure
sustained runtime CPU or end-to-end terminal latency. The fixed-viewport resize
fixture can invoke `tput` without a terminal; child CPU is outside the process
CPU sample. The existing trace is in `../compact-selection/diagnostics/size-trace`.
The unmet sustained typing/burst target documented in `../poll-timeout` remains
open. No frozen acceptance criterion has changed.

## Reproduction

Use nextest for all tests. Build all measured versions before starting the
serial runner. `performance/measure.py` records the exact local paths and order;
update those paths to retained binaries when reproducing on another host.
Compressed metadata, raw samples, commands, logs and comparisons are beside it.

```sh
cargo nextest run --profile ci -p harness-tui --all-features
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tui --all-features --ignore-default-filter -E 'binary(p0_03_pty_recorded) | binary(p1_04_pty_recorded)'
cargo nextest list --release -p harness-tui --all-features --test rewrite_performance_test --list-type binaries-only --message-format json
cargo metadata --locked --format-version 1
HARNESS_TUI_REFERENCE_FRAMES=/tmp/parts-main HARNESS_TUI_PLAN_FRAMES=/tmp/parts-plans HARNESS_TUI_WRAP_FRAMES=/tmp/parts-wrap cargo nextest run --profile ci -p harness-tui --all-features --test rewrite_reference_test
HARNESS_PARITY_RENDER_ARTIFACT_DIR=/tmp/parts-motion cargo nextest run --profile ci -p harness-tui --all-features --test reference_parity_render_test -E 'test(chat_and_tool_bullets_animate_without_recoloring_labels_or_reflowing_text) | test(all_tool_families_keep_reference_columns_through_streaming_and_disclosure)'
node scripts/qa/render-recorded-frames.mjs SELECTED_FRAME_DIR .omo/evidence/parts-selected
```

The selected frames and producer metadata are in the browser manifests.
`frames/` and `motion/` contain the complete final raw comparisons. The final
rewrite still needs broader source replacement, sustained runtime and
long-session signoff, supported-feature review, and final independent approval.
No new platform or live-provider verification is claimed here.

# Borrowed event inspection

Based on `6b732a69`. Healthy session inspection now borrows canonical source
events and the unsettled tail. It no longer retains a second event vector after
settlement. The retained prefix offset preserves the existing event cap across
settlement and later appends. The cap still limits inspection, not the canonical
history required for validation and replay.

Incomplete inline child slices and rejected histories retain an owned inspection
buffer. Invalid replacements remain deduplicated for navigation, while canonical
validation receives the original unfiltered input. Snapshots, fork and clone
intents contain the retained view. Activity lookups partition each borrowed slice;
rare APIs requiring a contiguous slice use `Cow`. Clone validation and dispatch
reuse that value. Backend contracts, dependencies and painting are unchanged.

Two existing tests are extended: retention across settlement and rejection, with
caps of zero and five; and public navigation through rejected live/replacement
histories. The first cap extension passes on unchanged production in
`retention-reference.log`; its exact test patch is retained. Clearing the rejected
replacement's inspection buffer leaks a duplicate sequence and fails the final
mutation check (`invalid-red.log`, `invalid-mutation.patch`). The mutation was
restored before all final checks. An earlier test incorrectly assumed replacement
reset the selected cursor; `initial-inspection-test.log` records that fixture
mistake. The corrected test explicitly navigates to the start.

One redundant private compaction-injection test is removed; the following
badge/summary behavior test already requires the same injected part. Production
code grows by 47 lines; inline tests grow by 26 net lines and integration tests by
24. The source tree has 557 Rust files / 169,250 lines including inline tests.
This is a storage migration; the remaining state engine, whole-history
presentation rebuild and lower formatters still need replacement.

## Verification

The final serial run passes 1,681 tests with seven configured skips. Seven gated
PTY checks, scoped all-target/all-feature Clippy, workspace check, formatting and
suite gates pass. The earlier `full.log` also passed; `interrupted-full.log` was
stopped to correct the cursor assumption. The initial Clippy log records an
assertion-style warning, corrected without a lint allowance.

All 543 recorded states and eight Unicode-plan states exactly match the preceding
candidate, including cells, styles, cursor, input and intents. The original golden
matrix remains unchanged, with the existing R8 correction explicit. Two fresh
paired xterm captures have identical ANSI and PNG bytes. Terminal snapshots match
except that the wide browser capture has one extra asynchronous render (4→5).
This is fixed-frame replay, not an animation-cadence or terminal-latency result.
The wide candidate screenshot was visually inspected.

```sh
HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/tui-event-history-journeys HARNESS_TUI_REFERENCE_FRAMES=/tmp/tui-event-history-frames HARNESS_TUI_PLAN_FRAMES=/tmp/tui-event-history-plan-frames cargo nextest run --profile ci -p harness-tui --all-features -j1
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tui --all-features --run-ignored all --test p0_01_pty_recorded --test p0_02_pty_recorded --test p0_03_pty_recorded -j1
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
node scripts/qa/render-recorded-frames.mjs /tmp/tui-event-history-xterm/reference .omo/evidence/tui-rewrite/event-history/reference --source-root /home/urbanbreach/.codex/worktrees/tui-reference/agent-harness
node scripts/qa/render-recorded-frames.mjs /tmp/tui-event-history-xterm/candidate .omo/evidence/tui-rewrite/event-history/candidate
```

The xterm input directories contain the two corresponding `browser/*/*.ansi.gz`
files decompressed. Producer metadata is preserved in their manifests.
`source.json`, `source.patch.gz`, binary receipts and `files.json` bind source,
executables and the published artifacts.

## Release measurements

All builds use the unchanged public benchmark, release mode and all features.
Each workload has three serial runs of 200 measured frames, plus a separate glibc
`memusage` run. Non-startup workloads retain 1,000 turns / 4,000 events. Compilation,
checks and browser captures finished before the final original/candidate runs.
The preceding candidate was measured on clean `6b732a69` before production edits;
its executable is retained too. This earlier before/after comparison is sequential,
not interleaved. Raw samples, allocation logs and all three binary hashes are kept.

```sh
cargo nextest list --release -p harness-tui --all-features --test rewrite_performance_test --message-format json
python3 scripts/measure-tui-rewrite.py --root REFERENCE --output /tmp/tui-event-history-reference --allocations
python3 scripts/measure-tui-rewrite.py --output /tmp/tui-event-history-candidate --allocations
```

| Workload | p95 original / candidate (µs) | p99 original / candidate (µs) | CPU ms/frame original / candidate | RSS KiB original / candidate | Allocated bytes original / candidate |
|---|---:|---:|---:|---:|---:|
| stream-1000 | 8,805 / 2,555 | 9,372 / 2,639 | 4.80 / 1.65 | 51,580 / 35,732 | 744,450,542 / 378,157,979 |
| resize-1000 | 9,691 / 5,684 | 10,200 / 5,890 | 5.60 / 2.10 | 65,968 / 42,136 | 670,820,536 / 364,755,885 |

Against the original, streaming RSS falls 30.7% and resize RSS 36.1%. Both also
pass the original frozen RSS limits of 36,122.8 and 46,244.8 KiB. Allocations fall
49.2% and 45.6%, passing their frozen limits. Streaming p95/p99 improve about 71%;
resize improves about 41–42%. These are accumulated rewrite gains, not gains
attributable to this slice alone.

Against the preceding candidate, streaming RSS falls 9.5% (39,464→35,732 KiB)
and resize RSS 8.2% (45,916→42,136 KiB). Peak heap falls 13.6% and 11.0%, while
total allocation is effectively unchanged: initial loading still constructs the
inspection buffer before successful validation releases it. This slice removes
retained duplication; it does not remove that construction cost.

Regressions remain visible in the full six-workload reports. Scrolling p99 rises
399→435 µs versus the preceding candidate, and coarse CPU 0.35→0.40 ms/frame.
Resize p99 rises 5,807→5,890 µs and CPU 2.00→2.10 ms/frame. Startup construction
rises 197→213 µs, while its first frame falls 1,239→1,055 µs. Against the paired
original, scrolling p99 rises 383→435 µs and typing CPU 0.30→0.35 ms/frame.
Static p99 results remain within the paired-reference allowance; short-run CPU
has 0.05 ms/frame tick granularity and is not a sustained-runtime conclusion.

The frozen earliest limits are unchanged. Streaming p95/p99 pass; resize and the
four static p99 limits still fail. Frozen CPU limits also fail: stream 1.65 exceeds
1.54 ms/frame, and resize 2.10 exceeds 1.96, despite improving over the slower
paired original. Both frozen timing and resource comparisons are published.
The previously observed host timing shift remains unexplained. Source reduction,
remaining implementation replacement and sustained runtime targets are unmet.

All timed byte counts and oldest retained screens match. All before/candidate
post-timing diagnostics match too. The original/candidate scroll diagnostic alone
retains the documented R8 blank-gap difference. These measurements cover event
or input handling, preparation, paint, diff and ANSI encoding to a counting sink;
they make no fresh end-to-end terminal, idle-runtime or live-provider claim.

Independent source and evidence review approved this storage migration for commit.
The clone path's duplicate copy was removed during review. Final review recomputed
all 54 timing runs and 18 allocation reports and checked the source, executables,
frame comparisons and artifact hashes; `review.txt` records the scope. The whole
rewrite remains unfinished.

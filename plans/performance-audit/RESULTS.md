# Performance audit implementation results

Measured 2026-09-06 on the audit host. All six ranked candidates are implemented.
The settlement follow-up removes one redundant source-event clone and characterizes
all three finish boundaries; it does not replace canonical reducers or change
settlement triggers. The audit's rejected/deferred architectural changes remain
rejected/deferred, not silently implemented.

## Outcome

| Surface | Fresh baseline | Modified | Change |
|---|---:|---:|---:|
| Hot render, 2,000 turns | 2.327 ms | 0.152 ms | 93.5% lower |
| Dirty ingest + render, 1,000 turns | 20.720 ms | 9.123 ms | 56.0% lower |
| Dirty ingest + render, 2,000 turns | 36.259 ms | 16.456 ms | 54.6% lower |
| Glob tool, 5,000 files | 36.681 ms | 8.383 ms | 77.1% lower |
| Broad grep tool, 5,000 files | 139.235 ms | 94.644 ms | 32.0% lower |
| Index append + persist, 2,000 rows | 9.909 ms | 8.411 ms | 15.1% lower |
| SSE 64 KiB frame / 64-byte chunks | 21.904 ms | 0.109 ms | about 201x faster |
| Three settlements, 2,000 turns | 237.569 ms | 225.707 ms | 5.0% lower |

These are fixture measurements, not live-provider or end-to-end input-latency claims.
Sparse grep did not show a meaningful improvement. Long-session settlement remains
expensive, as predicted by the audit; removing one clone is not an incremental
projection rewrite. The remaining dirty-frame semantic build still scans history.

## Changes and protected contracts

1. **Transcript semantics:** on a measured-layout miss, reuse sections from the
   existing four-entry cache when app identity, render key, theme, and surface match.
   Width still keys geometry. The actual-render regression checks semantic build
   counts, both scrollbar widths, cold-render cell equality, selection rows, links,
   and wide/combining characters after dirty content changes.
2. **Breadcrumb Git:** store a separate current-directory label at construction,
   not a recorded run-workspace label. Rendering only borrows it. Runtime refreshes
   outside rendering every five seconds, including while idle, and redraws only
   on a changed label. Supplied-clock tests cover deadlines and replay suppression;
   an injected discovery counter proves repeated startup/live renders do not probe
   and an explicit refresh changes both displayed branches. Initial construction
   still discovers the current directory, including when constructing replay state.
   The interval bounds probe frequency, not subprocess duration under a blocked OS.
3. **Glob:** use stable `sort_by_cached_key(Reverse(mtime))`; metadata fallback,
   exclusions, containment, sort-before-limit, and exact counts are unchanged.
   Existing ordering coverage now also checks tied mtimes and missing-file ties.
4. **History index:** use compact JSON only. Schema, row ordering, per-commit
   updates, cross-process locking, atomic replacement, mode 0600, and file/directory
   syncs are unchanged.
5. **Grep:** share entry collection but render the overflow artifact's lines only,
   without constructing the discarded human display. Inline limits, context,
   exact counts, exclusions, artifact formatting and digests are preserved. Existing
   integration tests now compare complete artifacts for match-limit and byte-limit
   overflow. The second scan and full-artifact materialization remain.
6. **SSE:** scan only appended bytes plus a three-byte overlap after unsuccessful
   delimiter search; reset after every drained frame, including ignored frames.
   Tests exercise mixed delimiters, chunk splits, ignored frames, UTF-8 splits,
   unterminated EOF data, and allocation reuse. No live payloads were collected;
   the large-frame result establishes synthetic scaling, not typical provider gain.
7. **Settlement:** pass the staged owned vector through construction using `Cow`,
   eliminating the second clone. The initial staging clone and assignment only
   after successful projection preserve failure atomicity. Tests compare each
   provider-finish, assistant-finish, and task-completion boundary with a fresh
   projection; invalid appends preserve canonical state, source history and TUI
   generation, and a subsequent valid core append succeeds.

## Reproducible A/B method

- Baseline: a pristine detached checkout of **`9abbd54f`**, freshly compiled.
  The old audit executable was retained, but its rlib dependency set could no
  longer be relinked reliably; it is not used for the tables in this report.
- Both sides use the same updated probe, including the new 1%-matching-file grep
  corpus. `build-probe.sh` selects exact dependency artifacts from Cargo JSON and
  includes the SSE module from the selected checkout, not accidentally from the
  modified tree. Both use locked release builds and the audit's standalone
  `opt-level=3`, fat-LTO, single-codegen-unit link flags. No Cargo profile changed.
- Linux x86_64, Intel i7-12800H, 20 logical CPUs; rustc 1.98.0
  (`88d9e12ae`, LLVM 22.1.8). CPU affinity, clocks, thermal state and unrelated host
  load are not controlled. No instrumentation tools were installed.
- **180 JSON records:** 30 workload/size combinations, two versions, three rounds.
  Workloads run serially without our builds/tests alongside them. Each workload is
  paired before/after; the second round reverses version order. This reduces, but
  does not eliminate, drift and is not fully randomized benchmarking.
- Tables report the median of three invocation medians. Cold render, append-turn,
  and the sum of the three settlements are each single measurements per invocation;
  their tables report the median of those three observations. The probe retains
  the original per-invocation sample counts. Raw sorted samples are in
  [comparison.jsonl](comparison.jsonl); sub-microsecond results round down to zero.
- TUI remains `Terminal<TestBackend>` at 120x40, five durable events per historical
  turn, ordinary memory caps and about 240 bytes of assistant text, without tools.
  This measures frame composition, not terminal transport or display latency.
- Index/glob/grep fixtures are page-cache-warm Btrfs files under
  `target/perf-artifacts`. Session-surface fixtures in this follow-up use the
  runner's default temporary directory under the user cache, also Btrfs, rather
  than the original audit's `/tmp` tmpfs. Both A/B sides use the same locations.
- Index append/persist retains the original synthetic repeated-event journal and
  fingerprint fixture. It measures persistence, not valid full-coordinator replay.

Run from the modified repository root, with no competing benchmark/build processes:

```bash
# Select a fresh path outside the working tree for the pristine baseline.
git worktree add --detach /tmp/harness-perf-baseline 9abbd54f
bash plans/performance-audit/build-probe.sh /tmp/harness-perf-baseline target/perf-artifacts/audit-probe-before
bash plans/performance-audit/build-probe.sh . target/perf-artifacts/audit-probe-after
bash plans/performance-audit/compare-probes.sh target/perf-artifacts/audit-probe-before target/perf-artifacts/audit-probe-after > target/perf-artifacts/comparison-new.jsonl
```

The actual baseline checkout for this run was in the agent's temporary directory.
Build manifests are retained beside each executable as `.cargo.jsonl`. SHA-256:

```text
before: 337a64e502946ec7e5ff3a8a0443b8ac602f2091368400cda811ba85a1cf50f8
after:  fed7c783f671afbd3bce1053a5883fefd897efb3ccfa87cc821571256a89ca9c
raw:    ab2951296b63a278a6e91b71a458d6b0484f398f8024311050c9ecb38dc43bb6
```

## Detailed measurements

### TUI

All values are milliseconds; each pair is **baseline / modified**.

| Turns | Cold render | Hot render | Dirty ingest + render | Three settlements total |
|---|---:|---:|---:|---:|
| 100 | 16.500 / 13.519 | 2.324 / 0.111 | 2.811 / 0.400 | 8.105 / 6.859 |
| 500 | 74.468 / 70.501 | 2.332 / 0.132 | 9.111 / 2.923 | 45.871 / 43.890 |
| 1,000 | 123.530 / 115.119 | 2.376 / 0.146 | 20.720 / 9.123 | 106.524 / 103.427 |
| 2,000 | 141.551 / 123.614 | 2.327 / 0.152 | 36.259 / 16.456 | 237.569 / 225.707 |

Direct workspace discovery is essentially unchanged: **1.865 / 1.867 ms**.
It was removed from repeated rendering, not globally cached or made faster.
Cold/load costs should not be inferred from hot-frame improvements; initialization
now owns the initial breadcrumb discovery.

### Filesystem tools

Whole-call medians in milliseconds, **baseline / modified**. Broad grep has
50 matches per file; sparse grep has the same file sizes but only every 100th file
matches. Inline match limit is 100. Artifacts remain in the excluded sessions tree.

| Files | Glob | Broad grep | Sparse grep |
|---|---:|---:|---:|
| 100 | 0.802 / 0.204 | 3.027 / 2.232 | 0.376 / 0.385 |
| 1,000 | 9.426 / 1.553 | 28.165 / 20.694 | 7.425 / 7.247 |
| 5,000 | 36.681 / 8.383 | 139.235 / 94.644 | 37.819 / 38.620 |

Sparse changes range from roughly -2.4% to +2.4%; do not claim a general grep
speedup. The isolated 5,000-path sort probe still records **62,598 comparator-path
metadata calls versus 5,000 cached-key calls**. These are instrumented isolated
sorts, not syscall tracing of the production tool. Production now uses the same
standard-library at-most-once key extraction with its relative-path join preserved.

### Index and projection

| Rows | Index bytes, baseline / modified | Append + persist ms, baseline / modified |
|---|---:|---:|
| 100 | 95,220 / 67,608 | 2.494 / 2.243 |
| 500 | 477,220 / 339,208 | 4.162 / 3.984 |
| 1,000 | 954,720 / 678,708 | 5.764 / 5.086 |
| 2,000 | 1,912,720 / 1,360,708 | 9.909 / 8.411 |

Compact index size is about **29% lower**, independent of latency noise.
The raw `serialize` field continues to measure pretty serialization deliberately;
`compact.serialize` measures compact serialization. Neither replaces the measured
whole append/persist result.

| Turns | Full canonical ms, baseline / modified | Append five-event turn ms, baseline / modified |
|---|---:|---:|
| 100 | 1.473 / 1.488 | 2.061 / 1.768 |
| 500 | 7.282 / 7.278 | 10.352 / 9.254 |
| 1,000 | 15.349 / 15.925 | 21.032 / 19.266 |
| 2,000 | 33.034 / 33.145 | 43.788 / 39.657 |

At 2,000 turns, separately timed legacy/conversation/transcript reducers were
17.898/3.665/5.370 ms before and 17.883/3.704/5.333 ms after. Source-vector cloning
was 1.767 / 1.720 ms. These are independent component calls, not additive attribution
of a settlement. Full reconstruction is effectively unchanged; the append path
benefits from moving its owned vector. Presentation enrichment and canonical
reconstruction still dominate settlement; no allocation/lock/queue profile or
changed batching policy is claimed.

### SSE

Milliseconds, **baseline / modified**. Zero below means less than one microsecond.

| Frame data bytes | 64-byte chunks | 4,096-byte chunks |
|---|---:|---:|
| 256 | 0.001 / <0.001 | <0.001 / <0.001 |
| 16,384 | 1.462 / 0.028 | 0.066 / 0.017 |
| 65,536 | 21.904 / 0.109 | 0.725 / 0.099 |
| 262,144 | 351.577 / 0.353 | 5.825 / 0.207 |

### Existing session surface benchmark

Five fresh serial baseline/modified repetitions, 120 sessions x 6 turns:

| Measurement | Baseline samples ms | Modified samples ms |
|---|---|---|
| List | 30 / 24 / 24 / 25 / 24 | 25 / 24 / 24 / 23 / 24 |
| Session search | 10 / 10 / 10 / 10 / 10 | 10 / 10 / 10 / 10 / 10 |
| Reopen | 0 / 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 / 0 |

No meaningful change is established for this small corpus. All result counts and
reopen identity checks passed. Artifacts: `target/perf-artifacts/session-{before,after}-{1,2,3,4,5}/large-session-surfaces.json`.

## Validation and manual QA

- `cargo fmt --all -- --check` and `cargo check --workspace`: passed.
- Clippy with all targets/features and `-D warnings` for all four modified crates:
  passed. Workspace-wide Clippy is blocked by the unchanged `unreachable!` at
  `crates/harness/src/runtime_catalog/astra_tests.rs:124`; it was not suppressed.
- Scoped release checks passed: 379 TUI `ui_` tests plus the runtime refresh test;
  34 filesystem unit tests; two full grep-artifact tests; SSE boundary/allocation
  tests; two index commit/concurrency tests; seven bounded-index CLI tests;
  32 core conversation-projection tests; four typed settlement tests; three
  incremental transcript tests; 30 core tests matching `workspace`.
- LSP diagnostics are clean on all modified workspace Rust files. The standalone
  probe is outside Cargo's workspace and LSP reports it as unlinked; both actual
  rustc builds succeeded. Bash LSP is unavailable and was not installed; both
  scripts passed `bash -n`, and their build/matrix paths were executed successfully.
- `cargo nextest run --profile ci --workspace --all-features`: **4,570 passed,
  eight failed, ten skipped**. Each failure was reproduced on pristine `9abbd54f`:
  two shipped-config count assertions (3 versus 2), the composed-prompt snapshot,
  generated-catalog partition count (3395 versus 3394), three dashboard snapshots
  (68 versus 80 operator probes), and the motion wake assertion (None versus 133ms).
  No failing test was removed, weakened, or snapshot-updated. Logs:
  `target/perf-artifacts/audit-nextest.log` and `audit-baseline-failures.log`.
- `scripts/test-lanes.sh perf`: **six tests passed**, artifact freshness passed.
  The first build exceeded the command's 20-minute timeout; resuming with a larger
  timeout completed successfully. Canonical artifacts are under
  `target/perf-artifacts/audit-perf-lane`.
- `bash scripts/harness-qa-dogfood.sh --self-test`: passed; evidence under
  `artifacts/qa-evidence/20260906-self-test`.
- Personally exercised the newly built CLI/TUI in tmux: help, invalid option
  rejection, successful deterministic golden-path run, interactive edit permission
  approval, completed transcript/diff, resize from 120x40 to 80x24, and `/quit` back
  to the shell. In an isolated QA repository, changed the external branch from
  `qa-before` to `qa-after` while the TUI was open and observed the breadcrumb
  refresh. The 80x24 terminal capture is `target/perf-artifacts/audit-tui-80x24.txt`.
  A free-form `--mock` prompt correctly reported a missing fixture; the shipped
  deterministic scenario supplied the successful offline path instead.
- Independent Oracle source review approved the cache, parser, filesystem,
  projection and durability changes without a blocking finding.

No live-provider, full PTY/native-pixel signoff, cold-disk, allocation/retained-heap,
hardware-counter, LSP/MCP-load, or lock-contention claim follows from these checks.

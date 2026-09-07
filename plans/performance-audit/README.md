# Performance Audit

Source commit: `9abbd54f`. Follow-up: all six ranked candidates implemented, plus
the scoped settlement clone removal and finish-sequence characterization.
See [implementation results and baseline comparison](RESULTS.md) and
[raw A/B samples](comparison.jsonl). The original audit below describes the
unmodified baseline; its rejected/deferred architectural changes remain out of scope.

The largest measured interactive costs are dirty transcript rendering and durable
settlement in long sessions. The best small changes remove repeated semantic section
construction, Git subprocesses from rendering, and filesystem metadata calls from
glob comparisons. Preserve the documented canonical-projection and per-commit index
contracts rather than treating them as accidental overhead.

**Ranking**
Assumes interactive coding sessions with growing histories and ordinary provider
frames, not a workload dominated by unusually large SSE frames or broad grep output.
Ranks reflect expected user impact, frequency, fix size, and confidence. Current costs
are not promised savings. S means hours; M means roughly a day; L means multi-day.

| Rank | Candidate | Measured Current Cost / Evidence | Effort | Fix Risk | Confidence |
|---|---|---|---|---|---|
| 1 | Reuse semantic transcript sections across widths | Dirty ingest + render: 19.967 ms at 1,000 turns, 36.452 ms at 2,000; repeated section construction in sampled stacks | S-M | Medium | High on repeated work; medium on savings |
| 2 | Move Git discovery out of breadcrumb rendering | 1.720-1.763 ms per discovery; two Git commands in a repository | S-M | Medium | High |
| 3 | Cache glob sort keys with the standard library | Isolated 5,000-file sort: 24.994 -> 2.418 ms; metadata calls 62,598 -> 5,000 | S | Low | High; production A/B still required |
| 4 | Serialize the history index without pretty-printing | About 29% fewer bytes; component median differences about 0.6-1.0 ms at 1,000-2,000 rows | S | Low | High on component savings; medium on commit latency |
| 5 | Avoid unused display rendering for grep artifacts | Broad 5,000-file search: 132.985 ms; overflow rescans and builds an unused display string | M | Medium | High on waste; medium on savings |
| 6 | Incrementally search fragmented SSE buffers | 64 KiB frame: 23.785 ms with 64-byte chunks versus 0.476 ms with 4 KiB chunks | S-M | Medium | High on synthetic scaling; low on normal-workload impact |

Durable settlement is a separate high-cost investigation below, not a recommendation
to replace the canonical architecture. No production optimization was implemented or
benchmarked against a modified production version.

**Methodology**
- Linux x86_64, Intel i7-12800H, 20 logical CPUs; `rustc 1.98.0 (88d9e12ae 2026-08-18)`, LLVM 22.1.8.
- Workspace release tests reported optimized builds with debug information. The standalone probe links those release libraries with `opt-level=3`, fat LTO, and one codegen unit. Those probe link flags are not a proposed Cargo configuration change.
- Benchmarks ran serially. CPU affinity, clock frequency, thermal state, and background load were not controlled. Component benchmarks run in a fixed order, not randomized A/B order; treat small differences cautiously.
- Existing session fixtures use temporary directories on `/tmp` (tmpfs). Index/glob/grep probes use Btrfs under `target/perf-artifacts`. Newly created files are page-cache warm; this is not a cold-disk benchmark.
- TUI measurements use release `AppState` and `Terminal<TestBackend>` at 120x40. This exercises frame composition, not terminal encoding, PTY transport, display latency, or a live provider.
- The TUI fixture has five durable events per turn, about 240 bytes of assistant text, no tool calls, and ordinary default memory caps. Caps are not disabled at larger sizes. Long tool-rich sessions need their own follow-up measurement.
- Probe medians use five samples for projection/filesystem/SSE, ten for TUI hot/dirty rendering and live ingestion, twenty for index components, and thirty for workspace discovery. Load, cold render, and each settlement are single observations. Raw samples are printed by the probe.
- GDB samples wall stacks at approximately 7 ms intervals. They include setup/teardown and debugger effects, are not exclusive CPU percentages, and cannot measure CPU-cache misses. Debugger-run timings are excluded from the timing tables.
- `perf`, `heaptrack`, `valgrind`, `samply`, `strace`, and `hyperfine` were unavailable. No tools were installed and no system settings changed. Allocation counts, retained heap, hardware cache locality, actual lock contention, and live network latency remain unmeasured.

**Existing Baselines**
All of the following scoped release tests passed. These are local checks, not a full
workspace, all-feature, live-provider, or PTY signoff.

| Existing Check | Result | Limitation |
|---|---|---|
| Large-session surfaces, five repetitions | List 48/23/24/23/23 ms; search 10/10/10/10/9 ms; reopen rounded to 0 ms | 120 sessions x 6 turns, 3,960 events; search is the `session_search` tool, not necessarily the CLI search command |
| Resume-plan performance, five repetitions | Passed 200 ms local budget; whole process roughly 3-4 ms | 75 resume projections over 250 provider turns; process duration is not one projection's latency |
| `perf_resize_to_render`, five repetitions | p95 approximately 0.75-0.78 ms; detached anchor preserved | 10,000 synthetic blocks, alternating warmed widths 80/160; not a dirty-render benchmark |
| Incremental transcript performance target | All three tests passed | Incremental-composite work counters do not cover every semantic section rebuilt by the actual renderer |
| `cargo fmt --all -- --check` | Passed | Standalone audit probe is outside workspace Cargo targets |

Large-session artifacts remain at
`target/perf-artifacts/audit-{1,2,3,4,5}/large-session-surfaces.json`.
These audit artifacts do not substitute for fresh canonical perf-lane evidence.

**1. Transcript Sections**
- **Location:** `crates/harness-tui/src/ui_transcript.rs:448`, `render_measured_transcript_pane`; `:1050`, `with_measured_transcript_layout_for_width_on_surface`; `crates/harness-tui/src/ui_transcript_sections.rs:5`, `build_transcript_sections`; `:597`, `build_ordered_assistant_parts_from_events`.
- **Wasted work:** The scrollbar decision measures full width, then paint measures scrollbar-reduced width. Each cache miss independently calls the width-independent `build_transcript_sections(app)`. That rebuilds every visible activity before measured-section reuse is checked. Each activity filters the whole event vector at lines 616-620 and reverse-scans it for provider state at lines 271-295. The first filter has O(activities x retained events) scan work per rebuild.
- **Evidence:** At 1,000 turns, median live ingestion alone was 13 us, hot render 2.261 ms, and dirty ingestion plus render 19.967 ms. At 2,000 turns dirty rendering was 36.452 ms. A 100-dirty-frame GDB run collected 446 wall snapshots, 233 containing `build_transcript_sections`; stacks also landed on the full-event filtering predicate. Counts include other render phases and are not exclusive CPU attribution.
- **Minimal change:** On an exact-width layout-cache miss, first reuse semantic `sections` from an existing entry with the same app identity, render key, theme, and surface, regardless of width. Keep measured layout keyed by width. The existing bounded `TRANSCRIPT_LAYOUT_CACHE` already owns the sections; no new cache subsystem is necessary. An owned clone is an acceptable first implementation if cheaper than rebuilding; do not introduce shared ownership machinery before measuring it.
- **Expected impact:** Removes one of two complete semantic section builds when both widths are needed for the same render key. This does not halve the entire frame cost and does not remove the remaining history scans. A meaningful reduction in dirty-frame CPU is plausible, but no modified-renderer timing exists yet. Reprofile before adding per-activity semantic caching or event indexes.
- **Maintainability:** S-M, concentrated in the current layout-cache path. Keep the four-entry bound and existing invalidation model. There is no reason to alter the separate incremental transcript composite or canonical projection.
- **Regression risk:** Medium: stale content, fold/hover/selection changes, theme changes, scrollbar transitions, and live/replay identity separation. Reuse semantics, never geometry from a different width. Do not replace the existing event filter with binary search without proving monotonic ordering for every ingestion/recovery path.
- **Validation:** Extend an existing actual-render test in `crates/harness-tui/src/ui_transcript_tests.rs` to change a live turn with a scrollbar and compare output/selection against a cold render at both widths. Count semantic builds, not just rendered blocks. Run `cargo test --release --locked -p harness-tui --lib ui_transcript -- --test-threads=1`, the incremental performance target, and the `tui` probe at 100/500/1000/2000 turns. Measure cold and dirty frames as well as warmed resizes.

| Turns | Load, ms | Cold Render, ms | Hot Render Median, ms | Live Ingest Median, us | Dirty Ingest + Render Median, ms | Three Settlements, ms |
|---|---:|---:|---:|---:|---:|---|
| 100 | 3.450 | 16.916 | 2.164 | 5 | 2.552 | 3.189 / 2.616 / 2.321 |
| 500 | 20.114 | 72.538 | 2.191 | 10 | 8.940 | 18.257 / 17.572 / 15.705 |
| 1000 | 44.205 | 119.423 | 2.261 | 13 | 19.967 | 32.872 / 32.711 / 32.441 |
| 2000 | 96.985 | 131.353 | 2.214 | 20 | 36.452 | 77.248 / 75.759 / 69.979 |

**2. Render-Time Git**
- **Location:** `crates/harness-tui/src/app.rs:3508`, `AppState::startup_directory_branch_label`; `crates/harness-tui/src/ui_lifecycle.rs:205` and `:217`, startup/live breadcrumbs; `crates/harness-core/src/workspace.rs:14`, `WorkspaceEnvironment::current`, and `:89`, `RealWorkspaceGitProbe::git_text`.
- **Wasted work:** Every breadcrumb-label evaluation rediscovers the working directory's Git environment. Inside a Git repository this synchronously launches `git rev-parse --show-toplevel` and `git symbolic-ref --quiet --short HEAD` during frame composition.
- **Evidence:** Three 30-sample discovery runs had medians 1.749, 1.763, and 1.720 ms. GDB stopped in libc `poll` with a stack through `Command::output`, the workspace Git probe, and the breadcrumb render path. Hot release rendering still costs about 2.2 ms; test workspace overrides can hide this subprocess cost.
- **Minimal change:** Discover display metadata at startup/runtime refresh boundaries and have both breadcrumb callers consume stored labels. `AppState::workspace_context_labels` and `app/workspace_display.rs` already provide storage/formatting for run-workspace labels. Reuse that only when its directory meaning matches the breadcrumb; otherwise retain a separate current-directory snapshot. Do not globally cache `WorkspaceEnvironment::discover`, whose other callers may require freshness.
- **Expected impact:** Avoid roughly 1.7 ms of synchronous discovery per affected render on this host, plus two process launches. It is not 1.7 ms of measured CPU time. Branch changes still need refresh outside rendering; a permanent startup-only value silently changes behavior.
- **Maintainability:** S-M; local state/runtime plumbing and a pure accessor. Avoid a filesystem watcher or generic cache framework unless a concrete refresh requirement demands one.
- **Regression risk:** Medium: current directory versus recorded workspace, external branch changes, detached HEAD, Git unavailable, home-shortened paths, and replay labels. Preserve those semantics and display sanitization.
- **Validation:** Extend existing breadcrumb/workspace-label tests with an injected discovery call counter: repeated renders must make no new probe calls, while an explicit refresh updates the displayed branch. Run `cargo test --release --locked -p harness-tui --lib ui_lifecycle -- --test-threads=1` and `cargo test --release --locked -p harness-core --lib workspace -- --test-threads=1`. Compare the `workspace` probe with hot `tui` rendering; the direct workspace probe itself should remain unchanged.

**3. Glob Sort Keys**
- **Location:** `crates/harness-tools/src/fs_glob.rs:127`, `sort_paths_by_mtime_desc`, and `:135`, `path_mtime`; shared caller `collect_matching_glob_paths` at line 111. Both native `glob` and the filesystem tool route through this collector.
- **Wasted work:** Sorting performs `metadata` and `modified` on both paths in every comparator call, including a path join. Modification times can be extracted once per path.
- **Evidence:** The real 5,000-file tool call had a 34.178 ms median. An isolated same-fixture sort changed from 24.994 to 2.418 ms using cached keys, with measured metadata calls falling from 62,598 to 5,000. At 1,000 files the isolated sort changed from 7.124 to 0.428 ms and calls from 18,346 to 1,000.
- **Minimal change:** Replace the comparator with `paths.sort_by_cached_key(|path| std::cmp::Reverse(path_mtime(workspace_root, path)))`. Both stdlib sorts are stable, preserving incoming order for equal keys. Keep the existing UNIX-epoch fallback for metadata failures and all walker/path checks.
- **Expected impact:** About 22.6 ms less isolated sort work in the 5,000-file fixture; approximately 10x faster sorting there. This is not a measured 10x tool speedup. The probe uses absolute `PathBuf` values rather than production relative strings/path joins, and whole-tool savings require A/B measurement.
- **Maintainability:** S; one stdlib operation, no dependency. Cached sorting uses extra O(files) key/index storage. Modification times become a consistent per-sort snapshot rather than repeated racing observations.
- **Regression risk:** Low: equal mtimes, disappearing files, fallback keys, ordering before limit, and exact total counts. The existing probe asserts equal outputs with distinct mtimes; that alone does not cover ties.
- **Validation:** Extend the existing mtime-ordering test in `fs_glob.rs` with equal timestamps and verify stable input order. Run `cargo test --release --locked -p harness-tools --lib fs_glob -- --test-threads=1`, then `glob` probes at 100/1000/5000 files. Check both tool latency and metadata-call count without weakening exclusions or symlink containment.

**4. Compact Index JSON**
- **Location:** `crates/harness-core/src/session/history_index.rs:102`, `write_history_index`, specifically `serde_json::to_vec_pretty` at line 103; `:62`, `persist_committed_history_row`; `crates/harness-core/src/coord/event_helpers.rs:600`, `append_built_event`.
- **Wasted work:** Each committed event locks, reads/parses, replaces one row, then serializes and atomically installs the entire shared catalog. Pretty-printing adds bytes and serialization work to that required path. The lock is held over parse/write/sync; its contention was not measured.
- **Evidence:** With 1,000 rows, pretty JSON was 954,720 bytes versus 678,708 compact; read/parse medians 2.143 versus 1.761 ms, serialization 0.552 versus 0.344 ms. With 2,000 rows, bytes were 1,912,720 versus 1,360,708; read/parse 4.649 versus 4.153 ms, serialization 1.199 versus 0.681 ms. The probe asserts equivalent decoded JSON.
- **Minimal change:** Use `serde_json::to_vec(index)` in `write_history_index`. Leave schema version, row ordering, per-commit updates, error handling, 0600 creation, atomic rename, file/directory syncs, and cross-process locking unchanged.
- **Expected impact:** Approximately 29% fewer index bytes and component median differences of 0.590 ms at 1,000 rows and 1.014 ms at 2,000. These sums are not measured transaction improvements. Sync cost and full-index O(sessions) work remain; use a whole-commit A/B before making latency claims.
- **Maintainability:** S; one serializer substitution. JSON consumers remain compatible, but manually inspecting a single-line file is less convenient. Do not introduce a database, shard format, background writer, or cache invalidation protocol for this change.
- **Regression risk:** Low: consumers unexpectedly depending on textual whitespace. Existing readers deserialize JSON. Preserve concurrent-row merge behavior, current fingerprints, malformed-index recovery, and warm-list zero journal opens.
- **Validation:** Run `cargo test --release --locked -p harness-core --lib canonical_commit_updates_history_index_before_first_list`, `cargo test --release --locked -p harness-core --lib concurrent_commit_updates_do_not_lose_rows`, and `cargo test --release --locked -p harness --test replay_sessions_cli_test part_16_bounded_history_index_test`. Repeat the `index` probe and existing session-surface benchmark on the same filesystem.

The following unmodified transaction timings are from a separate serial probe run:

| Catalog Rows | Index Bytes | Read/Parse Median, ms | Pretty Serialize Median, ms | Append + Index Persist Median, ms | Maximum, ms |
|---|---:|---:|---:|---:|---:|
| 100 | 95,220 | 0.163 | 0.063 | 2.501 | 2.845 |
| 500 | 477,220 | 1.495 | 0.294 | 3.837 | 4.959 |
| 1000 | 954,720 | 2.008 | 0.551 | 5.827 | 30.764 |
| 2000 | 1,912,720 | 4.325 | 1.186 | 9.426 | 19.902 |

The index fixture repeats a serialized event to grow a temporary journal and changes
its fingerprint. It isolates persistence work, not valid-event replay or the full
coordinator's journal durability path. A GDB `fsync` breakpoint recorded 65 calls for
`index 120`, including setup; do not interpret that as 65 per event.

**5. Grep Artifact Work**
- **Location:** `crates/harness-tools/src/fs_grep.rs:121`, `FsGrepTool::call`, overflow branch at lines 160-167; `:332`, `collect_grep_matches`, especially lines 383-394. Native `GrepTool` delegates through the same implementation.
- **Wasted work:** A truncated content search walks and reads files again to produce the full artifact. That second `collect_grep_matches` also builds a potentially large human display string, then its caller discards everything except `.lines`. Display and artifact lines separately format the same entries. Exact counts and complete overflow artifacts are legitimate requirements, not wasted work by themselves.
- **Evidence:** Corrected probes with 50 matching lines/file and inline limit 100 had medians 3.038 ms for 100 files, 27.193 ms for 1,000, and 132.985 ms for 5,000 (250,000 matching lines). In a separate 118-snapshot wall profile, 93 snapshots included the tool call and 65 included the overflow collection call at line 160; formatting and allocation frames appeared repeatedly. This does not mean all 65 samples are removable.
- **Minimal change:** Separate display rendering from the common match collection so the full-artifact branch only formats the lines it consumes. Retain the existing inline display, exact counts, context handling, and artifact content. Reprofile before considering a more invasive single-pass artifact writer; do not retain an unbounded full result merely to remove the second read.
- **Expected impact:** Fewer large temporary strings and formatting operations on broad/truncated searches. No percent reduction or heap-byte saving is established. File scanning and artifact writing remain, so 133 ms is the current whole-call cost, not the expected saving.
- **Maintainability:** M; a focused collector/render boundary change, not a replacement grep engine or a new external `rg` dependency.
- **Regression risk:** Medium: context merging, byte versus match limits, Unicode, exact counts, stable ordering, non-UTF-8 skipping, head limits, artifact digests, and workspace exclusions. Preserve secret/path handling at the tool boundary.
- **Validation:** Extend `crates/harness-tools/tests/native_grep_truncation_presentation_test.rs` to compare the entire overflow artifact with existing expected content. Run `cargo test --release --locked -p harness-tools --test native_grep_truncation_presentation_test` and `cargo test --release --locked -p harness-tools --lib fs_grep -- --test-threads=1`. Repeat `grep` probes with broad matches and a mostly-no-match corpus before claiming a general improvement.

An earlier probe placed artifacts in a searchable fixture directory and recursively
matched its own output. Those measurements were discarded. The current probe writes
under `.agent-harness/sessions/artifacts`, an existing walker exclusion. A source-line
GDB breakpoint on `read_utf8_lines` recorded zero hits in an optimized build; it is
not evidence of zero reads or a valid read-call count.

**6. Fragmented SSE**
- **Location:** `crates/harness-providers/src/openai/sse.rs:19`, `next_sse_event`, and `:51`, `sse_frame_boundary`.
- **Wasted work:** After each transport chunk, delimiter search starts at the beginning of the accumulated buffer. A frame of B bytes arriving in fixed chunks of C bytes can cause O(B squared / C) scan work before its terminating delimiter arrives.
- **Evidence:** The probe includes the original parser module directly and verifies the emitted data length. Median timings below show the current parser's sensitivity to fragmentation; they include chunk-vector allocation but exclude JSON parsing and network waits.
- **Minimal change:** If real provider frame/chunk distributions justify it, keep a local scan offset in `next_sse_event`; after an unsuccessful scan, recheck only the previous three trailing bytes plus appended bytes. Three bytes cover a split four-byte delimiter. Reset after draining a frame, including frames that emit no data. Keep earliest-delimiter ordering, UTF-8 validation, and EOF behavior unchanged.
- **Expected impact:** Makes delimiter scanning approximately linear in accumulated bytes for fragmented large frames. Typical small frames cost around a microsecond in this probe, so normal interactive gains may be negligible. This is a conditional candidate, not a recommendation to prioritize synthetic worst cases.
- **Maintainability:** S-M; one local cursor plus boundary coverage. No parser dependency, unsafe scanning, custom SIMD, or buffer-layout rewrite.
- **Regression risk:** Medium: mixed CRLF/LF/CR delimiters, split delimiters/UTF-8, multiple frames in one chunk, comments/empty frames, trailing data at EOF, and transport errors. Preserve current input-buffer allocation reuse.
- **Validation:** Extend the existing `next_sse_event_uses_the_earliest_mixed_delimiter` test with table-driven splits and retain `next_sse_event_reuses_the_input_buffer_allocation`. Run `cargo test --release --locked -p harness-providers --lib openai::sse::tests`; compare the `sse` probe across both frame size and chunk size. Collect only frame/chunk sizes for real-workload evidence, never raw provider payloads or reasoning.

| Frame Data Bytes | 64-Byte Chunks Median, ms | 4096-Byte Chunks Median, ms |
|---|---:|---:|
| 256 | 0.001 | <0.001 |
| 16,384 | 1.451 | 0.058 |
| 65,536 | 23.785 | 0.476 |
| 262,144 | 345.672 | 5.966 |

**Settlement Investigation**
- **Location:** `crates/harness-core/src/session/projection.rs:230`, `CanonicalSessionProjection::apply_events`; `crates/harness-tui/src/app/session_projection.rs:416`, `settle_durable_events`; `app/session_projection/settled_presentation.rs:34`, `rebuild_settled_presentation`; `crates/harness-tui/src/runtime_live_updates.rs:130`, `drain_with_limit`.
- **Cost:** `apply_events` clones retained source events, appends new ones, and rebuilds the composed canonical facade; `from_parts` owns another source-event copy. TUI settlement then rebuilds presentation and clones canonical transcript/run-summary values. The fixture settles separately on provider-finished, assistant-finished, and task-completed events.
- **Evidence:** Three successive 1,000-turn settlements took 32.872/32.711/32.441 ms, about 98 ms combined; at 2,000 turns they totaled about 223 ms. The runtime's 8 ms/16-update budget is checked between updates, so it cannot preempt a single long settlement. Queue growth and input latency were not directly measured.
- **Minimal next change:** First extend the existing settlement benchmark/test with the complete three-event finish sequence. Profile individual reducers and presentation enrichment separately. A narrowly scoped removal of a redundant source-event clone is worth an A/B, but must preserve `apply_events` failure atomicity. Do not replace the composed facade, redefine settlement triggers, or batch across permission/terminal events without a separate design decision.
- **Expected impact:** Unknown for a safe implementation change. Full source-event cloning alone measured 0.765 ms at 1,000 turns and 1.665 ms at 2,000, so removing one clone cannot explain away a 33-77 ms settlement. Large savings would require broader work than a clone cleanup.
- **Maintainability:** S for characterization, potentially L for incremental reducers or changed batch boundaries. This audit does not authorize that rewrite.
- **Regression risk:** High for broad changes: canonical authority, append validation, transient replacement, error atomicity, compaction, legacy history, permission responsiveness, and immediate post-ingest reads.
- **Validation:** `cargo test --release --locked -p harness-tui --test typed_runtime_event_settle_test`; extend the existing `live_settlement_projects_once_without_replaying_each_durable_event` case, then compare incremental results and errors with a fresh `CanonicalSessionProjection::from_event_history`. Use both `projection` and `tui` probe modes; the resume-plan-only budget is not sufficient evidence.

| Turns / Events | Full Canonical Median, ms | Source Vec Clone Median, ms | Legacy Adapter Median, ms | Conversation Median, ms | Transcript Median, ms | Apply One Five-Event Turn, ms |
|---|---:|---:|---:|---:|---:|---:|
| 100 / 502 | 2.102 | 0.088 | 0.751 | 0.147 | 0.225 | 1.994 |
| 500 / 2502 | 7.327 | 0.401 | 3.808 | 0.777 | 1.171 | 9.930 |
| 1000 / 5002 | 14.795 | 0.765 | 7.562 | 1.577 | 2.286 | 19.656 |
| 2000 / 10002 | 33.662 | 1.665 | 16.538 | 3.525 | 4.935 | 40.297 |

Component measurements are separate calls, not an exhaustive or additive breakdown
of one full projection. The appended-turn column is a single observation per size.

**Rejected / Deferred**
- Removing per-commit index updates: rejected. `docs/architecture/sessions-and-replay.md:179` explicitly specifies them; coordinator tests require an index before the first list and concurrent writers must not lose rows. Reader recovery is not permission to make the live index arbitrarily stale.
- Collapsing focused canonical reducers into a single monolithic reducer: rejected as a default optimization. `docs/architecture/sessions-and-replay.md:66` deliberately specifies a composed facade with independently evaluated pure reducers.
- Skipping journal/index syncs or weakening cross-process locks: rejected. Persistence guarantees are not optional performance overhead. The measured work is substantial; actual lock contention is not established.
- Lock-free channels, atomics, or blanket `spawn_blocking`: deferred. No contention/starvation evidence justifies changing these boundaries. LSP wrappers already use `spawn_blocking`; MCP intentionally reuses a serialized session. First remove known synchronous repeated work and measure queue delay under load.
- A new HTTP connection pool: rejected. Provider/tool transports already reuse `reqwest::Client`. No live TLS/network profile was collected, so transport dominance or retry costs are unknown.
- Broad clone/enum-layout/allocator changes: deferred. Source cloning was timed, but allocation counts, retained heap, and cache misses were not measured. Do not trade simpler ownership for speculative locality.
- Hand-written SIMD, ASM, unchecked indexing, or unsafe buffer manipulation: rejected. No measured arithmetic/byte primitive requires it, and workspace unsafe-code denial stays intact. Existing optimized dependency paths, including BLAKE3's vectorized implementation, already appeared in samples.
- Claiming canonical reconstruction is a per-token provider hot path: rejected. The audited production context route includes compaction context building; no per-token invocation frequency was established.
- Treating warmed resize or short-session list tests as proof of long streaming responsiveness: rejected. The direct dirty-render and settlement probes exercise materially different work.

**Coverage Limits**
The performance survey covered CLI/session surfaces, core coordination/persistence/
projection, TUI rendering/ingestion, provider streaming/transport, native filesystem/
subprocess tools, and deterministic test fixtures. It was hotspot-focused, not a
line-by-line proof over every crate. No correctness/security-wide audit, live provider
benchmark, PTY signoff, LSP/MCP load test, allocator profile, cold-disk run, lock-contention
stress test, or hardware-counter profile was performed. No claims about those areas
follow from passing the scoped tests.

**Reproduction**
Run from the repository root, without parallel benchmark processes. The initial build
and existing baseline commands were:

```sh
cargo test --release --locked -p harness-core --test perf_test -p harness --test perf_sessions_surface_test -p harness-tui --lib --test transcript_incremental_performance_test --no-run
cargo test --release --locked -p harness-core --test perf_test -- --nocapture --test-threads=1
cargo test --release --locked -p harness --test perf_sessions_surface_test -- --nocapture --test-threads=1
cargo test --release --locked -p harness-tui --lib perf_resize_to_render -- --nocapture --test-threads=1
cargo test --release --locked -p harness-tui --test transcript_incremental_performance_test -- --nocapture --test-threads=1
```

Repeated original baseline executions used the already-built test binaries. The
follow-up builder resolves exact rlib paths from Cargo's JSON output rather than
selecting arbitrary duplicate artifacts, and includes the SSE source from the
selected checkout. The unused `collect_body_text` warning comes from including that
module; no source lint setting was weakened. See RESULTS.md for a pristine baseline
build and interleaved A/B reproduction.

```sh
bash plans/performance-audit/build-probe.sh . target/perf-artifacts/audit-probe-after
```

The probe requires `target/perf-artifacts` to exist. Its temporary filesystem fixtures
are removed on exit. Each invocation prints one JSON record with raw timing samples.
Build the changed crate and relink this probe before any future production A/B; an
old executable still contains the old code.

```sh
target/perf-artifacts/audit-probe-after projection 1000
target/perf-artifacts/audit-probe-after tui 1000
target/perf-artifacts/audit-probe-after index 1000
target/perf-artifacts/audit-probe-after workspace
target/perf-artifacts/audit-probe-after glob 5000
target/perf-artifacts/audit-probe-after grep 5000
target/perf-artifacts/audit-probe-after sse 65536 64
target/perf-artifacts/audit-probe-after sse 65536 4096
AUDIT_DIRTY_SAMPLES=100 gdb -q -batch -x plans/performance-audit/sample.py --args target/perf-artifacts/audit-probe-after tui 1000
gdb -q -batch -x plans/performance-audit/sample.py --args target/perf-artifacts/audit-probe-after grep 5000
AUDIT_BREAK=fsync gdb -q -batch -x plans/performance-audit/count.py --args target/perf-artifacts/audit-probe-after index 120
```

For implementation signoff, retain scoped behavioral checks above, then run the
workspace's normal gates. These broader gates were not run as part of this audit:

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --profile ci --workspace --all-features
scripts/test-lanes.sh perf
```

**Suggested Order**
Start with candidates 2 and 3 for small, isolated changes, then candidate 1 for long
streaming sessions. Candidate 4 is independently measurable and preserves commit
semantics. Candidate 5 depends on broad-search frequency; candidate 6 needs realistic
fragmentation evidence. Characterize the full settlement sequence before proposing
any change to canonical batching. None of these requires a new dependency or reduced
safety checks. Select candidates before expanding this audit into implementation plans.

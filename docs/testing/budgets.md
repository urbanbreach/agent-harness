# Performance budgets

These budgets cover the listed local fixtures. They do not establish performance
for larger production workloads. Run the performance lane to collect current measurements.

| Budget | Current local threshold | Evidence |
|---|---:|---|
| startup/readiness | 2s local startup smoke | `signoff-binary` startup stage |
| TUI render | warm 10,000-entry resize p95 below 8.333 ms; frame/resource comparisons measured separately | `cargo nextest run -p harness-tui` and the release perf profile |
| session resume | 200ms default local resume-plan budget | `crates/harness-core/tests/perf/resume_plan_perf.rs` |
| large-session list/reopen/search | measured local artifact, no fast long-session claim by itself | `crates/harness/tests/perf_sessions_surface_test.rs` |
| binary size | no enforced threshold | no measurement gate yet |

## Startup/readiness budget

Startup/readiness covers launching the binary far enough to parse config, initialize local metadata, and render/help/report readiness without provider network calls. The local smoke budget is 2 seconds on the Linux development machine. Evidence should come from `signoff-binary` stage artifacts, including `command.txt`, `stdout.txt`, `stderr.txt`, `status.txt`, and `verification.txt`.

## TUI render budget

The release resize contract requires p95 below the 8.333 ms budget for 120 Hz. Deterministic
tests also cover startup, overlays, permissions, replay, Unicode geometry, and selection.
`scripts/measure-tui-performance.py` measures complete render/diff/ANSI encoding, CPU time,
resident memory, and terminal bytes across synthetic workloads. `scripts/measure-tui-runtime.py`
measures the production event loop and writer through a real PTY, including a slow reader.
See [the fluidity measurements](../performance/tui-fluidity-2026-09-13.md) for before/after data,
workload sizes, commands, and the distinction between PTY throughput and physical display refresh.

## Session resume budget

`perf_project_resume_plan_large_completed_log_under_budget` checks session resume
against a 200 ms local threshold. Use `HARNESS_PERF_RESUME_PLAN_BUDGET_MS` to
override it for a local experiment. Release documentation must cite the measured
command and its artifacts.

`perf_large_session_list_reopen_and_session_search_write_artifact` generates 120
sessions with 6 turns each, or 3,960 events total. It measures `harness sessions
list`, `harness sessions reopen --json`, and the model-visible `session_search`
tool, then writes `large-session-surfaces.json` under `HARNESS_PERF_ARTIFACT_DIR`.

The artifact records corpus sizes, timings, returned counts, searched session
count, reopened run id, command hint, timestamp, and artifact-root provenance.
The measurements apply to this fixture.

## Binary size budget

There is no enforced binary-size gate. Do not claim a size target without a
measurement of the built binary and an artifact tied to that build.

## Perf lane

`scripts/test-lanes.sh perf` runs
`cargo nextest run --release --profile perf --workspace --all-features`. It sets
`HARNESS_PERF_ARTIFACT_DIR` to the stage artifact directory and then runs
`scripts/check-perf-artifacts.py --artifact-dir <perf artifacts>` in the
`perf_artifact_freshness` stage.

The lane checks the resume-plan budget, the large-session measurements, and the
freshness of `large-session-surfaces.json`. Missing artifacts or timings, stale
timestamps, incorrect schema versions, and provenance that does not identify
the perf lane all fail the check. Do not freeze a baseline file to make it pass.

## Evidence policy

Every performance claim in README or release docs must point to a fresh lane artifact under the perf stage directory (or be removed/softened). Do not reintroduce claim ledgers or PRD checkboxes.

## Engine simplification baseline

`bash scripts/engine-metrics.sh --output <path> --baseline 060ee1fd` writes an
`engine-metrics-v1` comparison artifact. It records production LOC, the frozen
session/conversation/transcript/projection/provider-context/compaction overlap, event and
compaction variants, reducer and `SIZE_OK` counts, plus a deterministic mock run's log bytes and
event count. It names the existing 120-session perf fixture for corpus/list measurements, but
marks corpus list/inspect and long-session context rebuild timing `unavailable` until their owner
surfaces produce a truthful artifact. The supplied baseline facts remain in the JSON; any
fresh-measurement disagreement is a drift signal, not a replacement baseline.

## Failed budgets

Budgets are checked against current commands and artifacts. Do not add JSON baselines or allowlists that grandfather old measurements. When a budget fails, investigate the implementation and revise unsupported claims.
Do not change the threshold merely to accept the result.

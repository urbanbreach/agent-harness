# V1 budgets

Budgets are local release-readiness gates, not production-class performance claims. Production-class large-corpus ratification remains final-slice work.

| Budget | Current local threshold | Evidence |
|---|---:|---|
| startup/readiness | 2s local startup smoke | `signoff-binary` startup stage |
| TUI render | deterministic snapshot/render tests complete without timeout | `cargo nextest run -p harness-tui` |
| session resume | 200ms default local resume-plan budget | `crates/harness-core/tests/perf/resume_plan_perf.rs` |
| large-session list/reopen/search | measured local artifact, no fast long-session claim by itself | `crates/harness/tests/perf_sessions_surface_test.rs` |
| binary size | documented only in this slice | final-slice binary artifact gate |

## Startup/readiness budget

Startup/readiness covers launching the binary far enough to parse config, initialize local metadata, and render/help/report readiness without provider network calls. The local slice budget is 2s for the smoke path on the Linux dev box. Evidence should come from `signoff-binary` stage artifacts, including `command.txt`, `stdout.txt`, `stderr.txt`, `status.txt`, and `verification.txt`.

## TUI render budget

TUI render budget is currently guarded by deterministic TestBackend/snapshot tests. The expected behavior is that startup, overlays, transcript render, permission state, model switcher, session picker, diff rendering, resume, and replay-failure states complete inside normal cargo nextest run timeouts without sleeps or live dependencies.

## Session resume budget

Session resume uses the local `perf_project_resume_plan_large_completed_log_under_budget` test. The default local threshold is 200ms and can be adjusted only through `HARNESS_PERF_RESUME_PLAN_BUDGET_MS` for explicit local experimentation. Release docs must cite the actual command and artifact provenance rather than this prose.

Large-session list/reopen/search measurement uses `perf_large_session_list_reopen_and_session_search_write_artifact`. The test generates a local corpus of 120 sessions, 6 turns per session, and 3,960 total events, measures `harness sessions list`, `harness sessions reopen --json`, and the model-visible `session_search` tool, then writes `large-session-surfaces.json` under `HARNESS_PERF_ARTIFACT_DIR`. The artifact records corpus sizes, measured timings, returned counts, searched session count, the reopened run id, command hint, timestamp, and artifact-root provenance. These measurements are local release-readiness evidence only; they do not ratify production-class long-session performance claims.

## Binary size budget

Binary size is recorded as a documented limitation in this slice. A final-slice gate should measure the built `harness` binary, write the size artifact under the lane root, and fail closed if the artifact is missing or stale. Until that exists, no release claim should state a binary size achievement.

## Perf lane

`scripts/test-lanes.sh perf` runs `cargo nextest run --profile perf --workspace --all-features`. The lane exports `HARNESS_PERF_ARTIFACT_DIR` to the perf stage artifact directory so perf tests can write fresh measurement artifacts beside the lane summary. It then runs `scripts/check-perf-artifacts.py --artifact-dir <perf artifacts>` in the `perf_artifact_freshness` stage. The current hard gates include the resume-plan performance test with `HARNESS_PERF_RESUME_PLAN_BUDGET_MS` override, the large-session surface artifact writer, and the freshness checker for `large-session-surfaces.json`. Missing artifacts, stale timestamps, wrong schema versions, missing timings, or provenance that does not point back to `scripts/test-lanes.sh perf` fail closed. Do not freeze a baseline file to make the lane pass.

## Evidence policy

Every performance claim in README or release docs must point to a fresh lane artifact under the perf stage directory (or be removed/softened). Do not reintroduce claim ledgers or PRD checkboxes.

## Anti-gaming policy

Budgets are checked against current commands and artifacts. Do not add JSON baselines or allowlists that grandfather old measurements. A failing budget means the code or the claim changes, not the gate.

# Claim evidence matrix

This matrix maps maintained release-facing claims to current evidence or explicit limitations.

| Requirement / claim text | Evidence type | Evidence pointer | Verification | Observed result | Provenance | Status |
|---|---|---|---|---|---|---|
| README first source build quick start commands (`--help`, `--version`, `config validate`, `doctor`, mocked `prompt`) are real CLI surfaces. | test/lane | `crates/harness/tests/binary_smoke.rs` | `scripts/test-lanes.sh signoff-binary` | Env-gated lane exercises the real binary from outside the repo and writes per-stage artifacts when run. | 2026-05-30 local lane runner | PASS |
| README deterministic mocked execution prompt path is explicitly scoped apart from live provider/auth proof. | test/docs | `README.md`; `crates/harness/tests/binary_smoke.rs` | `cargo test -p harness --test binary_smoke -- --ignored --exact` via `scripts/test-lanes.sh signoff-binary` | Mock prompt produces `Hello world` and an event log; README states this does not prove live transport health. | 2026-05-30 env-gated binary smoke | PASS |
| README doctor local readiness only claim is enforced by doctor JSON scope fields. | test/CLI | `crates/harness/tests/config_schema_cli/02_doctor_cli_reports_shipped_orchestration_health_test.rs` | `cargo test -p harness --test config_schema_cli_test doctor_cli_emits_json_report` | Doctor JSON reports `readiness_scope: local_readiness_only`, `provider_execution_proof: false`, and `no_network_probes: true`. | 2026-05-29 deterministic doctor test | PASS |
| README native tool catalog claim mirrors the runtime registry. | test | `crates/harness-tools/tests/native_tool_parity_matrix_test.rs` | `cargo test -p harness-tools --test native_tool_parity_matrix_test` | Registry/doc drift fails the test. | 2026-05-29 deterministic | PASS |
| `ast_grep_search` ships and `ast_grep_replace` is not shipped in this slice. | documented limitation | `docs/native-tool-catalog.md` | docs-reference tests | Replace remains final-slice work. | PRD §3.3 | PASS |
| OpenAI-compatible provider is the supported execution path. | docs/test | `docs/provider-support.md` | `cargo test -p harness --test config_docs_reference_test` | Provider categories documented; broader providers are metadata/reference. | deterministic docs | PASS |
| Release-facing speed/performance claims require fresh artifacts. | limitation | `docs/budgets.md` | `scripts/test-lanes.sh perf` | Final production-class ratification is final-slice; current local budgets are provisional. | local perf lane | PASS |
| Release-blocking lanes are source-derived from `scripts/test-lanes.sh` and classified separately from local development aids. | docs-reference test | `docs/release-blockers.md`; `crates/harness/tests/test_lanes_script_test.rs` | `cargo test -p harness --test test_lanes_script_test release_blocker_taxonomy_maps_categories_to_real_lanes` | Every blocker category maps to at least one declared lane; local aids and doctor-vs-roadmap scope are documented. | 2026-05-29 deterministic docs test | PASS |
| Perf budget evidence fails closed when required artifacts are missing or stale. | lane/script | `scripts/check-perf-artifacts.py`; `scripts/test-lanes.sh` | `scripts/test-lanes.sh perf` | Perf lane runs `perf_artifact_freshness` after nextest; checker requires `large-session-surfaces.json` schema, timestamp, measurements, and provenance. | 2026-05-29 local perf gate | PASS |
| Prompt, permission, compaction, and skill tests use faux/mock providers by default. | test policy | `scripts/check-test-suite-gates.py` | `scripts/test-lanes.sh quality-gates`; `python3 scripts/check-test-suite-gates.py --self-test`; temporary `HARNESS_LIVE_PROXY` deterministic-test probe | Quality-gates passed with `static_test_suite_gates` and `forbidden_branding` green; a temporary deterministic test that read `HARNESS_LIVE_PROXY` failed the `live-provider-env` gate. | 2026-05-29 quality-gates `target/test-lanes/20260529-130106/summary.txt` | PASS |
| Doctor roadmap/extension readiness is separate from runtime health. | CLI test | `crates/harness/src/doctor.rs`; `crates/harness/tests/config_schema_cli/02_doctor_cli_reports_shipped_orchestration_health_test.rs` | `cargo test -p harness --test config_schema_cli_test doctor_cli_json_reports_extension_roadmap_readiness_separately` | Doctor JSON contains `extension_roadmap_readiness` with `separate_from_runtime_health: true`, final-slice seams, post-V1 seams, and no network probes. | 2026-05-29 deterministic doctor test | PASS |
| Outside-repository smoke covers TUI startup and a tool-enabled mock path, not only doctor/config preflight. | env-gated lane | `crates/harness/tests/binary_smoke.rs`; `scripts/test-lanes.sh` | `scripts/test-lanes.sh signoff-binary` | Binary smoke runs PTY-backed `tui --mock --exit-on-finish` and deterministic `run --scenario golden_path`; artifacts include TUI stdout/stderr/status with `success=true` and tool prompt events. | 2026-05-30 signoff-binary | PASS |
| README support export claim is backed by the sessions export surface and redaction tests. | test/docs | `crates/harness/tests/replay_sessions_cli/08_sessions_export_test.rs` | `cargo test -p harness --test replay_sessions_cli_test sessions_export` | Support bundles include replay-derived metadata, doctor JSON, non-secret summaries, artifact indexes, redaction manifest, and secret scan status without raw secrets. | 2026-05-29 deterministic session export tests | PASS |
| README session inspection claim is backed by replay-derived model-visible session tools. | test/docs | `docs/sessions-and-replay.md`; `crates/harness-tools/src/session_tools.rs` | `cargo test -p harness --test config_docs_reference_test thin_v1_docs_cover_their_source_surfaces` | Docs-reference guard asserts `list`, `inspect`, `replay`, `continue`, `export`, `tree`, `fork`, and `clone`; session tools remain replay-derived and side-effect free. | 2026-05-29 docs-reference test | PASS |

Rows must include an evidence pointer and verification command before a roadmap box can be flipped. Empty evidence or stale artifacts fail closeout.

## Maintained claim phrase list

The drift guard tracks a small set of real release-facing phrases rather than a broad debt baseline:

- first source build
- deterministic mocked execution
- local readiness only
- native tool catalog
- OpenAI-compatible provider
- support export
- session inspection
- release-blocking lanes

## Freshness policy

Lane artifact rows must name the lane, command, timestamp, artifact root, and observed pass/fail status. A row that points to a missing artifact or a command that has not been rerun for the current slice is stale and cannot back a checked box.

## Limitations policy

If a claim is not backed yet, either soften the claim in README/docs or mark the matrix row as an explicit documented limitation. Limitations are acceptable; unverifiable success claims are not.

## Review checklist

Before final closeout, compare README and public docs against this table, run the docs-reference tests, and update the progress log with the observed result for every checked PRD or roadmap box.

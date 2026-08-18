# AGENTS: crates/harness-testkit/tests

## OVERVIEW
Owner tests for deterministic simulation/parity, TUI fidelity and PTY evidence, env-gated live/native signoff, receipts, source guards, secret scanning, and artifact provenance.

Read `../AGENTS.md` first. TUI shell/render contracts live in `../../harness-tui/AGENTS.md` and `../../harness-tui/tests/AGENTS.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Offline PTY | `pty_e2e.rs` | Custom target; single-threaded deterministic evidence and manifest copies. |
| Live proxy | `README.live-proxy.md`, `live_proxy_e2e.rs` | Ignored, env-gated preflight and narrow live signoff wrappers. |
| Native visual | `native_visual_e2e.rs` | Ignored local display/capture signoff; screenshot provenance only. |
| Simulation | `simulation_validator_test.rs`, `support/simulation_validator.rs` | Validates the matrix, events, reports, and expected artifacts. |
| Semantic parity | `parity_*_test.rs` | Cells, artifact schema, differential proof, motion/timing, scenarios, scheduler. |
| Fidelity scenarios | `tui_fidelity_{scenario,scenario_rejection,baseline}_test.rs` | Scenario schema, rejection paths, and pinned baseline identity. |
| Fidelity execution | `tui_fidelity_runner_test.rs`, `tui_fidelity_pty_observer_test.rs`, `support/tui_fidelity_runner.rs` | Dual-runtime PTY fixture, observer, cleanup, and presentation receipts. |
| Compare/aggregate | `tui_fidelity_{compare,aggregate,presentation_receipt}_test.rs` | Per-run gates, physical evidence, no-visible-gap, pinned multi-run aggregate. |
| Closure/verify | `tui_fidelity_{closure,matrix,task_gate,verify}_test.rs`, `support/tui_fidelity_verify_*.rs` | Small test targets delegate detailed cases to support modules. |
| Packet contracts | `packet2_fixture_server_test.rs`, `tui_fidelity_packet6_contract_test.rs`, `fixtures/tui_fidelity/` | Fixture server, sustained stream, and packet contracts. |
| Authority receipts | `binary_receipt_test.rs`, `reference_authority_receipt_test.rs`, `source_guard_test.rs` | Binary identity, pinned authority, mutation and source-guard failures. |
| Dependency/secret/focus | `tui_dependency_audit_test.rs`, `secretscan_test.rs`, `focus_region_test.rs` | Dependency inventory, artifact scanning, focus-region math. |
| Shared support | `support/` | Repo roots, fixtures, lifecycle cases, verification obligations, staging helpers. |

## LANE AND ENV CONTRACT
- Simulation runs through `scripts/test-lanes.sh simulation` so replay, evidence, validation, and secret scan stay coupled.
- PTY/native lanes are single-threaded; live/native tests remain ignored unless their explicit gates are set.
- Live gates: `HARNESS_LIVE_PROXY`, config/provider/model variables, optional variant.
- Native gates: `HARNESS_NATIVE_VISUAL`, `DISPLAY`, visual artifact/capture variables.
- Fidelity evidence/cache gates include `HARNESS_PACKET1_EVIDENCE_DIR`, `HARNESS_PACKET2_EVIDENCE_DIR`, `PACKET2_FIXTURE_EVIDENCE`, `TUI_FIDELITY_REFERENCE_CACHE` plus key, presentation trace, interaction queue, and run root.
- Lane artifact env files are authoritative; do not infer signoff inputs from the developer shell afterward.

## COMMANDS
```bash
scripts/test-lanes.sh simulation
scripts/test-lanes.sh signoff-pty
scripts/test-lanes.sh signoff-parity
scripts/test-lanes.sh signoff-packet2
```

## CONVENTIONS
- PTY is the deterministic headless fallback; live wrappers do not replace deterministic behavior owners.
- Native screenshots are provenance-checked local evidence, not portable pixel hashes.
- Delegator tests using `#[path]` keep detailed cases in `support/`; add cases to the owning support module.
- `tests/fixtures/tui_fidelity/` is testkit-owned; top-level `fixtures/` may be consumed across crates.
- Report artifact paths when claiming signoff; missing required display, authority, receipt, or cache inputs fail closed.

## ANTI-PATTERNS
- Do not parallelize PTY/native lanes or remove determinism/env guards.
- Do not assume run ids, cache paths, screenshot paths, or temporary roots are stable.
- Do not claim provider/tool-flow behavior from slim live wrappers or one fidelity layer alone.
- Do not edit renderer defaults, generated receipts, or artifact copies to make a lane pass.

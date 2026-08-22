# AGENTS: crates/harness-testkit/tests

## OVERVIEW
Owner tests for deterministic simulation and PTY evidence, env-gated live/native signoff, secret scanning, and artifact provenance.

Read `../AGENTS.md` first. TUI shell/render contracts live in `../../harness-tui/AGENTS.md` and `../../harness-tui/tests/AGENTS.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Offline PTY | `pty_e2e.rs` | Custom target; single-threaded deterministic evidence and manifest copies. |
| Live proxy | `README.live-proxy.md`, `live_proxy_e2e.rs` | Ignored, env-gated preflight and narrow live signoff wrappers. |
| Native visual | `native_visual_e2e.rs` | Ignored local display/capture signoff; screenshot provenance only. |
| Simulation | `simulation_validator_test.rs`, `support/simulation_validator.rs` | Validates the matrix, events, reports, and expected artifacts. |
| Dependency/secret/focus | `tui_dependency_audit_test.rs`, `secretscan_test.rs`, `focus_region_test.rs` | Dependency inventory, artifact scanning, focus-region math. |
| Shared support | `support/` | Repo roots, fixtures, lifecycle cases, verification obligations, staging helpers. |

## LANE AND ENV CONTRACT
- Simulation runs through `scripts/test-lanes.sh simulation` so replay, evidence, validation, and secret scan stay coupled.
- PTY/native lanes are single-threaded; live/native tests remain ignored unless their explicit gates are set.
- Live gates: `HARNESS_LIVE_PROXY`, config/provider/model variables, optional variant.
- Native gates: `HARNESS_NATIVE_VISUAL`, `DISPLAY`, visual artifact/capture variables.
- Lane artifact env files are authoritative; do not infer signoff inputs from the developer shell afterward.

## COMMANDS
```bash
scripts/test-lanes.sh simulation
scripts/test-lanes.sh signoff-pty
```

## CONVENTIONS
- PTY is the deterministic headless fallback; live wrappers do not replace deterministic behavior owners.
- Native screenshots are provenance-checked local evidence, not portable pixel hashes.
- Delegator tests using `#[path]` keep detailed cases in `support/`; add cases to the owning support module.
- Report artifact paths when claiming signoff; missing required display, authority, receipt, or cache inputs fail closed.

## ANTI-PATTERNS
- Do not parallelize PTY/native lanes or remove determinism/env guards.
- Do not assume run ids, cache paths, screenshot paths, or temporary roots are stable.
- Do not edit renderer defaults, generated receipts, or artifact copies to make a lane pass.

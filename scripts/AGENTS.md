# AGENTS: scripts

## OVERVIEW
Repository verification scripts: canonical lane runner, static gates, stress harness, coverage ratchet, and perf artifact freshness checks.

Read root `AGENTS.md` first. Lane semantics are documented in `../docs/testing.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Lane runner | `test-lanes.sh` | Canonical mode dispatch and artifact writer. |
| Stress harness | `stress-harness.sh` | Offline/live prompt stress lanes and binary reuse. |
| Live smoke pack (WS-L1) | `harness-qa-live-smoke.sh` | Fail-closed without `HARNESS_LIVE_PROXY*`; budgeted live PONG smoke + redacted evidence. Not tool matrix / freestyle / multi-provider / PTY. |
| Static test gates | `check-test-suite-gates.py` | Determinism, process-global state, real deps, snapshots, arrange/act/assert debt. |
| Branding/source-term gate | `check-forbidden-branding.py` | Forbidden source-brand terms and allowlist handling. |
| Coverage ratchet | `coverage-ratchet.sh` | `cargo-llvm-cov` line coverage artifact and baseline comparison. |
| Perf artifacts | `check-perf-artifacts.py` | Freshness/provenance checks for perf lane outputs. |
| Nextest profiles | Cargo nextest defaults and lane flags | Test isolation profiles: `default`, `ci`, `perf`, `process-global-state`. |

## LANE RULES
- `test-lanes.sh` writes `<artifact-root>/summary.txt`, `<artifact-root>/env.txt`, and per-stage `command.txt`, `stdout.txt`, `stderr.txt`, `status.txt`, `verification.txt`.
- Keep `--dry-run` behavior in sync with real stage shape; it is used to validate lane wiring without running expensive commands.
- Deterministic default lane is `fast`: `cargo fmt --all -- --check`, `cargo check --workspace`, and nextest `ci`.
- PTY/live/native lanes are explicit signoff lanes; do not fold them into default deterministic CI without updating docs/tests.
- Live/native env requirements must be recorded in lane artifacts and fail closed when required variables are missing.

## UPDATE TOGETHER
| Change | Also update |
|--------|-------------|
| Lane mode/stage | `../docs/testing/testing.md`, `crates/harness/tests/test_lanes_script_test.rs`, owner tests |
| Static gate rule | `../docs/testing/testing.md`, baseline JSON only when debt legitimately changes |
| Coverage ratchet | `../docs/testing/testing.md`, coverage baseline docs/artifacts |
| Perf artifact rule | `../docs/testing/budgets.md`, perf tests producing the artifacts |

## COMMANDS
```bash
scripts/test-lanes.sh fast --dry-run
scripts/test-lanes.sh quality-gates
scripts/test-lanes.sh fast
scripts/test-lanes.sh simulation
python3 scripts/check-test-suite-gates.py
python3 scripts/check-forbidden-branding.py
```

## ANTI-PATTERNS
- Do not add sleeps, global env/cwd mutation, real network/subprocess/PTY dependencies, or orphan snapshots to deterministic tests without gate coverage and docs.
- Do not make lanes claim evidence that is not written to artifacts.
- Do not hide test isolation issues by adding broad `process-global-state` exemptions.
- Do not edit static-gate baselines just to get green output.

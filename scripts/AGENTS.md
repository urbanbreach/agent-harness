# AGENTS: scripts

## OVERVIEW
Repository verification and evidence tooling: canonical lane runner, static gates, stress/coverage/perf helpers, offline dogfood, live smoke, TUI parity capture, and TUI fidelity guards.

Read root `AGENTS.md` first. Lane semantics are documented in `../docs/testing/testing.md`; the TUI fidelity evidence contract spans `../scripts/tui-fidelity/`, `../scripts/tui-parity/`, `../configs/tui-fidelity-*.json`, and `../docs/reference/`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Lane runner | `test-lanes.sh` | Canonical mode dispatch and artifact writer. Modes: `fast`, `integration`, `quality-gates`, `perf`, `coverage`, `simulation`, `signoff-binary`, `signoff-pty`, `signoff-live`, `signoff-native`, `signoff-parity`, `signoff-packet2`, `signoff-journeys`, `stress-offline`, `stress-live`, `all-deterministic`. |
| Stress harness | `stress-harness.sh` | Offline/live prompt stress lanes and binary reuse. |
| Live smoke pack (WS-L1) | `harness-qa-live-smoke.sh` | Fail-closed without `HARNESS_LIVE_PROXY*`; budgeted live PONG smoke + redacted evidence. Not tool matrix / freestyle / multi-provider / PTY. |
| Offline dogfood | `harness-qa-dogfood.sh` | Deterministic golden-path dogfood + gitignored QA evidence under `artifacts/qa-evidence/<date>-<slug>/`; run after product-touching changes. |
| Test-module stripper | `strip-cfg-test.sh` | Strips `#[cfg(test)]` modules to measure production lines. |
| Static test gates | `check-test-suite-gates.py` | Determinism, process-global state, real deps, snapshots, arrange/act/assert debt. |
| Branding/source-term gate | `check-forbidden-branding.py` | Forbidden source-brand terms and allowlist handling. |
| Coverage ratchet | `coverage-ratchet.sh` | `cargo-llvm-cov` line coverage artifact and baseline comparison. |
| Perf artifacts | `check-perf-artifacts.py` | Freshness/provenance checks for perf lane outputs. |
| TUI fidelity guards | `tui-fidelity/` | `source-guard.sh` (pinned reference/revision verify), `build-candidate.sh` (build harness-testkit `tui-fidelity` runner), `watchdog.sh` (bounded evidence gate). Consume `../configs/tui-fidelity-*.json`. |
| TUI parity capture | `tui-parity/` | `capture-*-l3.sh` scene captures, `generate-evidence-layers.py`, `compare-pixels.mjs`, web-terminal visual QA. Backs `signoff-parity` / `signoff-packet2`. |
| Nextest profiles | none | No repository-local `nextest.toml` or `[metadata.nextest]`; lanes currently pass `--profile ci` / `--profile perf`, so profile-argument changes must update scripts, CI, and testing docs together. |

## CONVENTIONS
- `test-lanes.sh` writes `<artifact-root>/summary.txt`, `<artifact-root>/env.txt`, and per-stage `command.txt`, `stdout.txt`, `stderr.txt`, `status.txt`, `verification.txt`.
- Keep `--dry-run` behavior in sync with real stage shape; it validates lane wiring without running expensive commands.
- Deterministic default lane is `fast`: `cargo fmt --all -- --check`, `cargo check --workspace`, and nextest `ci`.
- PTY/live/native lanes are explicit signoff lanes; do not fold them into default deterministic CI without updating docs/tests.
- Live/native env requirements must be recorded in lane artifacts and fail closed when required variables are missing.
- `signoff-parity` and `signoff-packet2` are fail-closed (missing manifest/env/reference binary/owners = FAIL) and own dual-binary cells/pixels/PTY acceptance; `../docs/testing/tui-signoff-manifest.v1.json` does not.

## UPDATE TOGETHER
| Change | Also update |
|--------|-------------|
| Lane mode/stage | `../docs/testing/testing.md`, `crates/harness/tests/test_lanes_script_test.rs`, owner tests |
| Static gate rule | `../docs/testing/testing.md`, baseline JSON only when debt legitimately changes |
| Coverage ratchet | `../docs/testing/testing.md`, coverage baseline docs/artifacts |
| Perf artifact rule | `../docs/testing/budgets.md`, perf tests producing the artifacts |
| TUI fidelity/parity tooling | `../configs/tui-fidelity-*.json`, `../docs/reference/`, harness-tui/testkit signoff owners |
| Dogfood/live smoke contract | `../docs/testing/testing.md`, root `AGENTS.md` dogfood note |

## COMMANDS
```bash
scripts/test-lanes.sh fast --dry-run
scripts/test-lanes.sh quality-gates
scripts/test-lanes.sh simulation
scripts/test-lanes.sh signoff-parity
python3 scripts/check-test-suite-gates.py
python3 scripts/check-forbidden-branding.py
bash scripts/harness-qa-dogfood.sh --self-test
```

## ANTI-PATTERNS
- Do not add sleeps, global env/cwd mutation, real network/subprocess/PTY dependencies, or orphan snapshots to deterministic tests without gate coverage and docs.
- Do not make lanes claim evidence that is not written to artifacts.
- Do not hide test isolation issues by adding broad `process-global-state` exemptions.
- Do not edit static-gate baselines just to get green output.
- Do not hand-edit `../configs/tui-fidelity-*.json` to force a signoff verdict; update the contract inputs with the runner and owners that consume them.

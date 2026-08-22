# AGENTS: crates/harness-testkit

## OVERVIEW
Deterministic test infrastructure: fakes and isolated workspaces, simulation validation, secret scanning, and signoff helpers.

Read root `AGENTS.md` first. Workflow-heavy test and signoff rules live in `tests/AGENTS.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Deterministic fakes | `src/fakes.rs` | Scripted command/HTTP/id sources and call recording. |
| Isolated workspaces | `src/workspace.rs` | Temp roots, manual clock, seeded ids, fixture paths. |
| Simulation contract | `src/simulation.rs`, `src/simulation/`, `src/bin/simulation_evidence.rs` | Matrix, event/report validation, fingerprints, artifact writers. |
| Secret hygiene | `src/secret_scanner.rs`, `tests/secretscan_test.rs` | Shared scanner plus env-gated artifact scans. |
| Other helper binaries | `src/bin/native_visual_helper.rs` | Local visual metadata generation. |

## CONVENTIONS
- Helpers under `src/` stay deterministic and reusable; env-gated provider/display behavior belongs in `tests/` or helper binaries.
- Simulation artifacts derive from `../../docs/testing/simulation-matrix.json`; preserve normalized summaries, fingerprints, and provenance.
- Secret scanning fails closed; do not allowlist real-looking credentials.
- `target/`, lane artifacts, copied screenshots, and runtime sessions are generated evidence, not source fixtures.

## TESTS
```bash
cargo nextest run -p harness-testkit
cargo nextest run -p harness-testkit --test simulation_validator_test
scripts/test-lanes.sh simulation
```
Follow `tests/AGENTS.md` for PTY, live, and native signoff commands.

## ANTI-PATTERNS
- Do not make deterministic helpers depend on network, display servers, wall-clock time, or host-specific paths.
- Do not claim native or PTY evidence without the matching lane artifacts.
- Do not mix live proxy assertions into simulation or fake-provider helpers.
- Do not edit generated receipts, evidence trees, sessions, or local `target/` outputs as source.

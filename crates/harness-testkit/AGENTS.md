# AGENTS: crates/harness-testkit

## OVERVIEW
Deterministic test infrastructure: fakes and isolated workspaces, simulation validation, semantic-cell parity, TUI fidelity runners, evidence receipts, secret scanning, and signoff helper binaries.

Read root `AGENTS.md` first. Workflow-heavy test and signoff rules live in `tests/AGENTS.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Deterministic fakes | `src/fakes.rs` | Scripted command/HTTP/id sources and call recording. |
| Isolated workspaces | `src/workspace.rs` | Temp roots, manual clock, seeded ids, fixture paths. |
| Simulation contract | `src/simulation.rs`, `src/simulation/`, `src/bin/simulation_evidence.rs` | Matrix, event/report validation, fingerprints, artifact writers. |
| Secret hygiene | `src/secret_scanner.rs`, `tests/secretscan_test.rs` | Shared scanner plus env-gated artifact scans. |
| Semantic-cell parity | `src/parity.rs`, `src/parity/`, `tests/parity_*_test.rs` | Cells, frame IO, identity, motion, provenance, exact comparison. |
| Fidelity scenarios | `src/tui_fidelity.rs`, `src/tui_fidelity/`, `src/tui_fidelity_baseline*.rs` | Scenario/checkpoint schema, substitution, baseline identity. |
| Fidelity runner | `src/tui_fidelity_runner.rs`, `src/tui_fidelity_runner/` | Dual-runtime PTY execution, cleanup, sidecars, presentation receipts. |
| Compare/aggregate | `src/tui_fidelity_compare.rs`, `src/tui_fidelity_compare/`, `src/tui_fidelity_aggregate.rs`, `src/tui_fidelity_aggregate/` | Per-run gates and pinned multi-run aggregation. |
| Closure/verification | `src/tui_fidelity_{closure,matrix,task_gate,verify}.rs` | Requirement closure, matrices, task gates, cache/deadline verification. |
| Evidence authority | `src/binary_receipt.rs`, `src/reference_authority_receipt.rs`, `src/tui_dependency_audit.rs` | Binary identity, reference authority, dependency provenance. |
| Fidelity CLI | `src/bin/tui-fidelity.rs`, `src/bin/tui_fidelity_commands/` | Compare, aggregate, closure, matrix, task-gate, and verify commands. |
| Other helper binaries | `src/bin/native_visual_helper.rs`, `src/bin/binary_receipt.rs` | Local visual metadata and receipt generation. |
| Fixtures | `fixtures/`, `tests/fixtures/tui_fidelity/` | Cross-crate mock/stress fixtures vs testkit-owned fidelity scenarios. |
| Owner tests | `tests/AGENTS.md` | PTY/live/native, parity/fidelity, receipts, source guards, and provenance. |

## CONVENTIONS
- Helpers under `src/` stay deterministic and reusable; env-gated provider/display behavior belongs in `tests/` or helper binaries.
- Simulation artifacts derive from `../../docs/testing/simulation-matrix.json`; preserve normalized summaries, fingerprints, and provenance.
- Fidelity comparisons keep cell, pixel, timing, motion, presentation, and authority gates separate; one passing layer does not imply another.
- Receipt and authority inputs fail closed on schema, digest, revision, path, or provenance mismatch.
- Secret scanning fails closed; do not allowlist real-looking credentials.
- `target/`, lane artifacts, copied screenshots, and runtime sessions are generated evidence, not source fixtures.

## TESTS
```bash
cargo nextest run -p harness-testkit
cargo nextest run -p harness-testkit --test simulation_validator_test
scripts/test-lanes.sh simulation
```
Follow `tests/AGENTS.md` for PTY, live, native, parity, and packet-specific signoff commands.

## ANTI-PATTERNS
- Do not make deterministic helpers depend on network, display servers, wall-clock time, or host-specific paths.
- Do not claim native, PTY, parity, or reference-authority evidence without the matching lane artifacts.
- Do not mix live proxy assertions into simulation or fake-provider helpers.
- Do not edit generated receipts, evidence trees, sessions, or local `target/` outputs as source.

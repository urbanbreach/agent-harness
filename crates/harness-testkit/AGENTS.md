# AGENTS: crates/harness-testkit

## OVERVIEW
Deterministic test support crate: fake providers/tools/commands, isolated workspaces, simulation evidence generation, secret scanning, and helper binaries for signoff lanes.

Read root `AGENTS.md` first. Test-file-specific PTY/live/native rules are in `tests/AGENTS.md`.

## STRUCTURE
```text
src/
├── fakes.rs                 # scripted command/http/id sources and call recording
├── workspace.rs             # isolated temp workspaces, manual clocks, fixture dirs
├── simulation.rs            # public simulation API and required artifact list
├── simulation/              # evidence, fingerprint, validation helpers
├── parity.rs                # TUI reference-parity L2 semantic cells + exact compare
├── parity/                  # cells, compare, frame_io, vt100 adapter
├── secret_scanner.rs        # artifact/cassette secret-pattern checks
└── bin/
    ├── simulation_evidence.rs
    └── native_visual_helper.rs
tests/                       # deeper AGENTS.md governs PTY/live/native test files
target/                      # generated local artifacts; not source
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Deterministic fakes | `src/fakes.rs` | Scripted command/http behavior, seeded ids, mismatch diagnostics. |
| Workspaces | `src/workspace.rs` | Temp root/session/artifact helpers and manual test clock. |
| Simulation evidence | `src/simulation.rs`, `src/simulation/`, `src/bin/simulation_evidence.rs` | Matrix validation, replay/report checks, artifact writers. |
| Secret hygiene | `src/secret_scanner.rs`, `tests/secretscan_test.rs` | Shared scanner plus env-gated artifact scans. |
| Semantic cell parity (L2/A-CELLS) | `src/parity.rs`, `src/parity/`, `tests/parity_cells_test.rs` | Exact SemanticFrame capture/compare; identity grapheme masks only; no SSIM. |
| Native visual helper | `src/bin/native_visual_helper.rs` | Local screenshot metadata helper used by native signoff. |
| E2E lanes | `tests/AGENTS.md`, `tests/*.rs` | PTY, live proxy, native visual, and simulation validator contracts. |
| Simulation contract | `../../docs/testing/simulation-matrix.json`, `tests/simulation_validator_test.rs` | Keep matrix, validator, and evidence outputs aligned. |

## CONVENTIONS
- Library helpers under `src/` should stay deterministic and reusable by crate tests.
- Put env-gated provider/display behavior in `tests/` or helper binaries, not in ordinary fake/workspace helpers.
- Simulation artifacts should be reproducible from the matrix and replay inputs; preserve normalized summaries and provenance files.
- Secret scanning should fail closed on suspicious artifacts; do not special-case real-looking credentials into allowlists.
- Native visual screenshots are local provenance evidence; PTY/simulation lanes are the deterministic proof surfaces.
- Treat `crates/harness-testkit/target/` and copied lane artifacts as generated evidence, not source fixtures.

## TESTS
```bash
cargo nextest run -p harness-testkit
cargo nextest run -p harness-testkit --test simulation_validator_test
cargo nextest run -p harness-testkit --test secretscan_test
scripts/test-lanes.sh simulation
```
For signoff-specific PTY/live/native commands, follow `tests/AGENTS.md`.

## ANTI-PATTERNS
- Do not make deterministic fakes depend on network, display servers, wall-clock time, or host-specific paths.
- Do not mix live proxy assertions into simulation or fake-provider helpers.
- Do not claim screenshot/native evidence from helper code without the matching signoff lane artifact.
- Do not edit generated evidence, session artifacts, or local `target/` outputs as source fixtures.

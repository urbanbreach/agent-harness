# AGENTS: docs

## OVERVIEW
Public contract, architecture, release evidence, and operator documentation for the harness. Docs here are tested claims, not loose notes.

Read root `AGENTS.md` first. Config-specific schema guidance lives in `../configs/AGENTS.md`; lane behavior lives in `../scripts/AGENTS.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Architecture/events | `architecture/architecture.md`, `architecture/sessions-and-replay.md` | Event schema, replay boundaries, session inspection, support export. |
| Config contract | `configuration/config.md`, `configuration/provider-support.md`, `configuration/starter-skills.md` | Public config keys, provider catalogs, skill discovery. |
| Tool contract | `tools/native-tool-catalog.md`, `tools/ast-grep-replace-safety-gate.md` | Native ids, permissions, mutation/replay/artifact behavior. |
| Permissions/privacy | `permissions/permissions.md`, `permissions/privacy-and-local-data.md` | Approval limits, local data, redaction, support bundles. |
| Testing/evidence | `testing/testing.md`, `testing/budgets.md` | Lane semantics, perf/coverage, evidence policy. |
| Simulation/TUI manifests | `testing/simulation-matrix.json`, `testing/tui-signoff-manifest.v1.json` | Machine-read validation inputs. |
| Parity/evidence contracts | `reference/` | TUI reference parity manifest, capability inventory, core-subsystem disposition, phase-0 audit receipt, and retired legacy parity-loop docs. Evidence contracts, not planning docs. |
| Generic agent/troubleshooting | `operations/generic-agent-and-tasks.md`, `operations/troubleshooting.md` | Generic task execution and local troubleshooting. |
| Extensions/migration | `operations/extension-strategy.md`, `operations/migration-notes.md` | Extension strategy and migration history. |
| Static gate baseline | `testing/test-suite-conventions-baseline.json` | Machine-read debt baseline for test-suite gates. |

## CONVENTIONS
- Every release/readiness claim needs concrete evidence: test name, lane artifact, or explicit unchecked/post-V1 status.
- Public contract docs are drift-tested; update owner tests when changing anchors or public names.
- Keep docs branding-safe and source-term-safe; `../scripts/check-forbidden-branding.py` is part of quality gates.
- Do not use docs to advertise behavior that runtime/config/tool catalog tests do not prove.
- JSON manifests in this directory are source inputs for validators; update validators and tests with schema/shape changes.
- Do not reintroduce PRD, progress, plan, roadmap, or claim-ledger markdown into this tree.
- `reference/` holds parity/evidence contracts and historical records. Retired parity-loop docs are historical non-acceptance records; the active reference authority is `../configs/tui-fidelity-reference-authority.json`.

## UPDATE TOGETHER
| Change | Also update |
|--------|-------------|
| Event/replay behavior | `architecture/architecture.md`, `architecture/sessions-and-replay.md`, event docs tests |
| Public config shape | `configuration/config.md`, `../configs/*.json`, examples, config docs/schema tests |
| Native tool catalog row | `tools/native-tool-catalog.md`, registry/catalog code, parity test |
| Test lane behavior | `testing/testing.md`, `../scripts/test-lanes.sh`, script tests |
| Simulation invariant | `testing/simulation-matrix.json`, testkit validator/evidence |
| TUI signoff flow | `testing/tui-signoff-manifest.v1.json`, harness-tui/testkit signoff tests |
| TUI fidelity evidence contract | `reference/`, `../configs/tui-fidelity-*.json`, `../scripts/tui-fidelity/`, `../scripts/tui-parity/`, owner signoff tests |

## TESTS
```bash
cargo nextest run -p harness --test config_docs_reference_test
cargo nextest run -p harness --test event_docs_reference_test
cargo nextest run -p harness-tools --test native_tool_parity_matrix_test
cargo nextest run -p harness-testkit --test simulation_validator_test
cargo nextest run -p harness-tui --test tui_signoff_manifest_test
cargo nextest run -p harness-testkit --test reference_authority_receipt_test
scripts/test-lanes.sh quality-gates
```

## ANTI-PATTERNS
- Do not reintroduce PRD/progress/plan/roadmap/claim-ledger docs.
- Do not claim PTY/live/native visual evidence without artifact provenance.
- Do not broaden descriptor-only extension seams into runtime plugin claims.
- Do not edit baseline debt files to bypass static gates without explaining the debt change.
- Do not treat `reference/` as planning authority or resume retired parity-loop instructions; treat its manifests as evidence contracts consumed by signoff owners.

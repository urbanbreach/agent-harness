# AGENTS: docs

## OVERVIEW
Public contract, architecture, release evidence, and operator documentation for the harness. Docs here are tested claims, not loose notes.

Read root `AGENTS.md` first. Config-specific schema guidance lives in `../configs/AGENTS.md`; lane behavior lives in `../scripts/AGENTS.md`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Architecture/events | `architecture.md`, `sessions-and-replay.md` | Event schema, replay boundaries, session inspection, support export. |
| Config contract | `config.md`, `provider-support.md`, `starter-skills.md` | Public config keys, provider catalogs, skill discovery. |
| Tool contract | `native-tool-catalog.md`, `ast-grep-replace-safety-gate.md` | Native ids, permissions, mutation/replay/artifact behavior. |
| Permissions/privacy | `permissions.md`, `privacy-and-local-data.md` | Approval limits, local data, redaction, support bundles. |
| Testing/evidence | `testing.md`, `budgets.md`, `claim-evidence-matrix.md`, `release-blockers.md` | Lane semantics, perf/coverage, evidence status. |
| Simulation/TUI manifests | `simulation-matrix.json`, `tui-signoff-manifest.v1.json` | Machine-read validation inputs. |
| PRDs/progress | `*-prd.md`, `*-progress.md`, `roadmap-v1.md` | Historical claims; keep evidence rows honest and dated. |
| Agents/troubleshooting | `agents-and-subagents.md`, `troubleshooting.md` | Agent routing, local troubleshooting. |
| Extensions/migration | `extension-strategy.md`, `migration-notes.md`, `desktop-distribution-surface-map.md` | Extension strategy, migration history, desktop surfaces. |
| Config restructure | `config-restructure-prompt.md`, `config-restructure-spec.md` | Historical config restructure specs. |
| Static gate baseline | `test-suite-conventions-baseline.json` | Machine-read debt baseline for test-suite gates. |

## CONVENTIONS
- Every release/readiness claim needs concrete evidence: test name, lane artifact, or explicit unchecked/post-V1 status.
- Public contract docs are drift-tested; update owner tests when changing anchors or public names.
- Keep docs branding-safe and source-term-safe; `scripts/check-forbidden-branding.py` is part of quality gates.
- Do not use docs to advertise behavior that runtime/config/tool catalog tests do not prove.
- JSON manifests in this directory are source inputs for validators; update validators and tests with schema/shape changes.
- Historical progress docs should not be rewritten to hide old limitations; add dated superseding notes instead.

## UPDATE TOGETHER
| Change | Also update |
|--------|-------------|
| Event/replay behavior | `architecture.md`, `sessions-and-replay.md`, event docs tests |
| Public config shape | `config.md`, `../configs/*.json`, examples, config docs/schema tests |
| Native tool catalog row | `native-tool-catalog.md`, registry/catalog code, parity test |
| Test lane behavior | `testing.md`, `../scripts/test-lanes.sh`, script tests |
| Simulation invariant | `simulation-matrix.json`, testkit validator/evidence |
| TUI signoff flow | `tui-signoff-manifest.v1.json`, harness-tui/testkit signoff tests |

## TESTS
```bash
cargo test -p harness --test config_docs_reference_test
cargo test -p harness --test event_docs_reference_test
cargo test -p harness-tools --test native_tool_parity_matrix_test
cargo test -p harness-testkit --test simulation_validator_test
cargo test -p harness-tui --test tui_signoff_manifest_test
scripts/test-lanes.sh quality-gates
```

## ANTI-PATTERNS
- Do not check roadmap/progress boxes without a matching evidence row.
- Do not claim PTY/live/native visual evidence without artifact provenance.
- Do not broaden descriptor-only extension seams into runtime plugin claims.
- Do not edit baseline debt files to bypass static gates without explaining the debt change.

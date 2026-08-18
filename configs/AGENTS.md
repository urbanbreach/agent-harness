# AGENTS: configs

## OVERVIEW
Generated schemas, starter runtime/TUI configs, extension manifest schema, provider catalog fixtures, and TUI fidelity evidence contract inputs. These files are public contract inputs.

Read root `AGENTS.md` first. Human-facing explanation belongs in `../docs/configuration/config.md`; runtime parsing lives in `crates/harness-core/src/config/`.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Runtime schema | `config.json` | Generated schema for `harness.json{,c}`. |
| TUI schema | `tui.json` | Generated schema for `tui.json{,c}`. |
| Starter runtime config | `harness.example.jsonc` | Canonical quick-start example and doctor/config validation fixture. |
| Starter TUI config | `tui.example.jsonc` | TUI-only defaults; keep separate from runtime config. |
| Provider catalogs | `provider-catalog.generated.json`, `provider-catalog.reference.jsonc` | Bundled generated catalog and larger reference fixture. |
| Extension schema | `extension-manifest.v1.schema.json` | Descriptor-only typed extension manifest schema. |
| TUI fidelity reference authority | `tui-fidelity-reference-authority.json` | Active canonical reference authority: pinned checkout, executable, source revision, binary sha. Consumed by `../scripts/tui-fidelity/source-guard.sh`, `signoff-parity`, and reference-authority owners. |
| TUI fidelity contract inputs | `tui-fidelity-requirement-inventory.json`, `tui-fidelity-coverage-manifest.json`, `tui-fidelity-closure-contract.json`, `tui-fidelity-dependency-cones.json` | Machine-read inputs for the harness-testkit `tui-fidelity` runner (`closure`, `matrix`, `verify` commands). |
| TUI fidelity reference receipt | `tui-fidelity-reference-binary-receipt.json` | Pinned evidence receipt for the reference binary; captured when the reference is rebuilt. |

## CONVENTIONS
- Treat `config.json`, `tui.json`, and `extension-manifest.v1.schema.json` as generated outputs. Regenerate through the owning Rust code path; do not hand-edit schema drift.
- TUI fidelity contract manifests are source inputs with `schema_version` headers; the reference-binary receipt is generated evidence. Update either through its owning runner/test path, never to force a green verdict.
- Runtime config is `harness.json{,c}`; TUI config is `tui.json{,c}`. Do not merge those surfaces.
- Examples should use canonical public keys, not compatibility aliases.
- `harness.example.jsonc` should remain a runnable first-run config with Codex OAuth-backed OpenAI-compatible defaults.
- `provider-catalog.generated.json` is bundled by the binary; generated updates need deterministic input or an explicit `models generate` run note.
- `provider-catalog.reference.jsonc` is validation/reference data, not runtime auto-discovery.

## UPDATE TOGETHER
| Change | Also update |
|--------|-------------|
| Runtime config field | `../docs/configuration/config.md`, core config parser/schema tests, examples, README |
| TUI config field | `../docs/configuration/config.md`, TUI config parser/tests, `tui.example.jsonc` |
| Provider catalog default | generated catalog code/tests, `../docs/configuration/provider-support.md`, README if first-run behavior changes |
| Extension manifest schema | `../docs/operations/extension-strategy.md`, `crates/harness-core` extension manifest tests |
| TUI fidelity contract manifest | `../scripts/tui-fidelity/`, `../scripts/tui-parity/`, `../docs/reference/`, harness-tui/testkit signoff owners |

## TESTS
```bash
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo run -p harness -- --config configs/harness.example.jsonc doctor --json
cargo nextest run -p harness --test config_schema_cli_test
cargo nextest run -p harness --test config_docs_reference_test
cargo nextest run -p harness-core --test extension_manifest_test
cargo nextest run -p harness-testkit --test reference_authority_receipt_test
cargo nextest run -p harness-testkit --test tui_fidelity_matrix_test
cargo nextest run -p harness-testkit --test tui_fidelity_runner_test
```

## ANTI-PATTERNS
- Do not broaden compatibility aliases into examples or docs as canonical names.
- Do not commit schema changes without regenerating and running drift tests.
- Do not put credentials, tokens, host-specific paths, or local MCP commands into starter configs.
- Do not claim generated catalog coverage for providers the runtime cannot execute.
- Do not hand-edit `tui-fidelity-*.json` to force a verdict green; update the contract inputs with the runner that owns them.
- Do not treat `tui-fidelity-reference-authority.json` as disposable; `source-guard.sh` and `signoff-parity` fail closed on a missing/invalid authority.

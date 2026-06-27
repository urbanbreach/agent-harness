# AGENTS: crates/harness-core/src/config

## OVERVIEW
Runtime/TUI config implementation: public contract, compatibility aliases, discovery, validation, model/provider registries, skills/LSP/MCP/hook settings, and schema generation.

Read `../../AGENTS.md` first. This directory owns parsing and validation, not command behavior or runtime execution.

## LOAD PIPELINE
1. `discovery.rs` resolves XDG/workspace/env/project paths and markdown agent/instruction assets.
2. `loader.rs` parses JSON5, translates the public surface, resolves references/assets, validates internals, then refreshes registries.
3. `public.rs` and `public/` define the documented public runtime/TUI schema and translate it into `HarnessConfig`.
4. `aliases.rs`, `provider.rs`, and `public.rs` merge compatibility aliases into canonical fields with conflict detection.
5. `validation.rs` enforces MCP, hooks, skill roots, LSP overrides, and path constraints.
6. `registries.rs` publishes resolved config snapshots for runtime code; keep registry mutation in loader finalization.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Public contract | `public/contract.rs`, `public.rs` | Canonical/compatibility/inert/unsupported key status and docs/schema metadata. |
| Agent profiles | `public/agents.rs` | Shipped profile defaults and profile merge semantics. |
| Discovery | `discovery.rs` | Config layers, project roots, instruction files, markdown frontmatter. |
| Loading | `loader.rs` | Parse → translate → validate → finalize registry sequence. |
| Providers/models | `provider.rs`, `model_types.rs`, `model_catalog.rs`, `model_selection.rs` | Provider options, variants, model profile fallback metadata. |
| Integrations | `integrations.rs`, `validation.rs` | MCP/remote search/LSP/hook/skill validation. |
| Defaults/registries | `defaults.rs`, `registries.rs` | Default values and global read-only runtime snapshots. |
| Tests | `tests.rs`, `tests/` | Discovery merge, public basics, agents, permissions/models, env assets, formatter. |

## CONTRACT RULES
- Public runtime config is `harness.json{,c}`; TUI config is `tui.json{,c}`. Do not merge those surfaces.
- Add canonical keys in all four places: `PublicRuntimeConfig`, `public/contract.rs`, generated schema expectations, and `docs/config.md` drift tests.
- Compatibility aliases are migration inputs only. Do not expose them as canonical examples/help/docs.
- Runtime schemas come from `harness_schema_pretty_json()` / `harness_tui_schema_pretty_json()`; do not hand-edit `configs/config.json` or `configs/tui.json`.
- Provider option aliases must use the conflict-detecting alias helpers. New transport variants need their own alias normalization.
- Registry refresh belongs to loader finalization; direct test mutation must clear the matching registry afterward.

## TESTS
```bash
cargo test -p harness-core --test mcp_config_test
cargo test -p harness-core --test model_variant_resolution_test
cargo test -p harness --test config_schema_cli_test
cargo test -p harness --test config_docs_reference_test
```

## ANTI-PATTERNS
- Do not add `#[serde(alias = ...)]` without representing the alias in the public contract.
- Do not change discovery precedence without updating `docs/config.md` and discovery merge tests.
- Do not validate by accepting unknown top-level keys; unsupported areas should fail explicitly.
- Do not make generated schema drift pass by editing JSON outputs alone.
- Do not put credential values or host-specific paths into defaults or examples.

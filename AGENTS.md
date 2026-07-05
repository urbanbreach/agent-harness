# PROJECT KNOWLEDGE BASE

**Generated:** 2026-06-27
**Commit:** `bf28ab0e`
**Branch:** `dev`

## OVERVIEW
Rust workspace for an event-sourced agent harness: CLI entrypoint, coordinator/runtime core, provider adapters, native tool surface, Ratatui TUI, runtime prompt assets, and deterministic plus opt-in signoff lanes.

## STRUCTURE
```text
agent-harness/
├── crates/harness/           # CLI binary/library: auth, config, doctor, models, prompt, run, replay, sessions, TUI handoff
├── crates/harness-core/      # coordinator, events, permissions, config, projections, lineage, hashline edits
├── crates/harness-providers/ # provider trait, OpenAI-compatible transport, mock provider, cassettes
├── crates/harness-tools/     # native tools: fs/edit/bash/task/web/code/lsp/mcp/session/control plane
├── crates/harness-tui/       # Ratatui app state, layout, overlays, transcript renderer, terminal signoff
├── crates/harness-testkit/   # deterministic fakes, workspaces, simulation, PTY/live/native evidence helpers
├── configs/                  # generated schemas, starter examples, provider catalogs
├── docs/                     # public architecture/config/testing/tool/session/release documentation
├── scripts/                  # lane runner, static gates, perf/coverage/stress helpers
└── .agent-harness/           # runtime agent profiles, prompt-family fragments, shipped skills, generated sessions
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| CLI behavior | `crates/harness/AGENTS.md` | `main.rs` stays a thin shim; command tests use `CliIo`/`CliDeps`. |
| Runtime invariants | `crates/harness-core/AGENTS.md` | Read before changing events, coordinator, permissions, config, replay, lineage, or compaction. |
| Coordinator internals | `crates/harness-core/src/coord/AGENTS.md` | Turn phases, task lifecycle, permissions, hooks, compaction, child sessions. |
| Config internals | `crates/harness-core/src/config/AGENTS.md` | Public contract, aliases, discovery, schema generation, registries. |
| Provider protocol | `crates/harness-providers/AGENTS.md` | Read before changing `ProviderStreamEvent`, request metadata, cassettes, or transport code. |
| Native tools | `crates/harness-tools/AGENTS.md` | Read before changing tool ids, schemas, path safety, bash, MCP, LSP, task/session tools. |
| TUI shell | `crates/harness-tui/AGENTS.md` | Read before touching app state, layout, transcript rendering, overlays, keybindings, or snapshots. |
| TUI app state | `crates/harness-tui/src/app/AGENTS.md` | AppState, session projection/stack, permissions, composer, model switcher. |
| Test helpers and signoff | `crates/harness-testkit/AGENTS.md`, `crates/harness-testkit/tests/AGENTS.md` | Deterministic fakes, simulation, PTY/live/native evidence, artifact provenance. |
| Runtime prompt assets | `.agent-harness/AGENTS.md` | Runtime-loaded agent profiles, prompt-family fragments, and skill packages. |
| Public docs | `docs/AGENTS.md` | Architecture, config, testing, tool catalog, session/replay, release evidence. |
| Public config and schemas | `configs/AGENTS.md` | Generated schemas, example configs, provider catalogs. |
| Build/test scripts | `scripts/AGENTS.md` | Lane runner, static gates, coverage/perf/stress scripts. |

## CODE MAP
| Area | Role | High-value tests |
|------|------|------------------|
| `harness-core::coord` | Single scheduling/event/permission/hook/lifecycle authority | `cargo nextest run -p harness-core --test coord_test` |
| `harness-core::event` | Event schema v1 and append-only envelopes | `cargo nextest run -p harness --test event_docs_reference_test` |
| `harness-core::config` | Runtime/TUI config loading, validation, public schema shape | `cargo nextest run -p harness --test config_schema_cli_test` |
| `harness-providers` | Streaming provider boundary, redacted metadata, cassette replay | `cargo nextest run -p harness-providers` |
| `harness-tools` | Built-in native tool surface and schema parity | `cargo nextest run -p harness-tools --test native_tool_parity_matrix_test` |
| `harness-tui` | App state, view models, renderer, shell geometry | `cargo nextest run -p harness-tui --test deterministic_render_test` |
| `harness-testkit` | Fakes, simulation evidence, visual/PTY helpers | `cargo nextest run -p harness-testkit --test simulation_validator_test` |

## FIRST-PARTY SEARCH SCOPE
- Include by default: `crates/`, `configs/`, `docs/`, `scripts/`, `.agent-harness/agents/`, `.agent-harness/prompt-families/`, `.agent-harness/skills/`, root manifests.
- Exclude by default: `target/`, `.git/`, `sessions/`, `artifacts/`, `.harness/`, `.gnhf/`, `.sisyphus/`, `.omx/`, `.omo/`, `.codex/`, `inspirations/`, `screenshot folder/`.
- Search `inspirations/`, `.codex/`, and `.omx/saved-dirty/` only when explicitly comparing reference implementations or recovering saved local state. Their `AGENTS.md` files are external/reference/cache content, not this project guidance.

## COMMANDS
```bash
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates
scripts/test-lanes.sh integration
scripts/test-lanes.sh all-deterministic
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo run -p harness -- --config configs/harness.example.jsonc doctor --json
```

Targeted signoff:
```bash
scripts/test-lanes.sh simulation
scripts/test-lanes.sh signoff-binary
scripts/test-lanes.sh signoff-pty
RUST_TEST_THREADS=1 cargo nextest run -p harness-testkit --test pty_e2e --test-threads 1
```

## CONVENTIONS
- Workspace lints deny `unsafe_code`, `dbg_macro`, and `todo`; clippy runs with `-D warnings`.
- Cargo workspace commands should be explicit: `--workspace` for all members, `-p <crate>` for scoped checks.
- `rust-toolchain.toml` pins stable plus `rustfmt` and `clippy`; CI also uses `cargo-nextest` and `cargo-llvm-cov`.
- Runtime config is `harness.json{,c}`; TUI config is `tui.json{,c}`. Keep the public contracts separate.
- Canonical permission names: `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp`.
- Canonical native tool ids include `read`, `list`, `glob`, `grep`, `edit`, `bash`, `task`, `background_output`, `batch`, `question`, `skill`, `webfetch`, `websearch`, `codesearch`, `lsp`.
- `task` calls require `prompt`, `run_in_background`, and `load_skills`; skills resolve before child spawn.
- `AGENTS.md` files are project guidance; `.agent-harness/agents/*.md` and `.agent-harness/prompt-families/*.md` are runtime prompt assets. Do not mix those layers.

## MANDATORY CODING SKILLS
- For any coding work in this repository, load `karpathy-guidelines` and `programming` before the first edit. Coding work includes implementation, bug fixes, refactors, tests, build scripts, schemas, generated-code maintenance, and AGENTS guidance edits.
- Delegated coding tasks must include the skills in `load_skills`; omit it only for pure read-only exploration, documentation-only lookup, or non-coding QA.

## UPDATE TOGETHER
| Change | Also update |
|--------|-------------|
| Public config keys or validation | `docs/config.md`, `configs/config.json`, `configs/tui.json`, example configs, config schema tests |
| Event variants or replay semantics | `docs/architecture.md`, `docs/sessions-and-replay.md`, event/replay docs tests |
| Native tool ids/schema/capability | `docs/native-tool-catalog.md`, `native_tool_parity_matrix_test`, permission docs as needed |
| Test lane behavior or evidence shape | `docs/testing.md`, `scripts/test-lanes.sh`, owner tests |
| Simulation invariants | `docs/simulation-matrix.json`, `simulation_validator_test`, simulation evidence, secret scan |
| Provider model catalog | `configs/provider-catalog.generated.json`, `configs/provider-catalog.reference.jsonc`, generated catalog docs/tests |
| Runtime prompt assets or shipped skills | `.agent-harness/AGENTS.md`, bootstrap/profile/skill discovery tests, prompt snapshots |
| Starter config defaults | `configs/harness.example.jsonc`, `configs/tui.example.jsonc`, README quick start |

## INVARIANTS
- Events are the source of truth; replay is side-effect free and derives from JSONL in contiguous `seq` order.
- Coordinator is the only event append, task scheduling, permission resolution, hook, compaction, and lifecycle authority.
- Permission checks precede tool execution; worker redelegation bypasses must remain blocked.
- Hashline edits validate anchors, reject overlaps, apply bottom-up, and write atomically.
- Provider-context compaction writes checkpoint artifacts/events and must not rewrite `events.jsonl`.
- Session inspection tools read replay-derived data only; they must not execute providers, hooks, MCP, network, or the CLI.
- Provider metadata persisted to events/artifacts must be redacted; never store raw requests, raw responses, auth headers, cookies, keys, PEM blocks, or hidden reasoning text.

## ANTI-PATTERNS
- Do not move runtime invariants into CLI, tools, providers, or TUI crates.
- Do not make replay execute tools, network calls, hooks, or provider work.
- Do not broaden compatibility aliases into the canonical public config contract.
- Do not treat runtime session/artifact directories as source.
- Do not claim PTY/live/native visual evidence without the matching lane and artifact provenance.

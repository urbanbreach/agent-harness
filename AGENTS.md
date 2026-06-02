# PROJECT KNOWLEDGE BASE

**Generated:** 2026-05-29
**Commit:** `974b473`
**Branch:** `dev`

## OVERVIEW
Rust workspace for an event-sourced agent harness: CLI entrypoint, coordinator/runtime core, provider adapters, built-in native tools, Ratatui TUI, and deterministic PTY/live/native verification lanes.

## STRUCTURE
```text
agent-harness/
├── crates/harness/           # CLI binary/library: tui, prompt, run, replay, sessions, config, models
├── crates/harness-core/      # event store, coordinator, permissions, config, projections, hashline edits
├── crates/harness-providers/ # Provider trait, OpenAI-compatible transport, mock/cassette replay
├── crates/harness-tools/     # native tool registry: fs/edit/bash/task/web/lsp/mcp/session/team
├── crates/harness-tui/       # Ratatui startup/live/replay shell and transcript renderer
├── crates/harness-testkit/   # fakes, workspaces, simulation, PTY/live/native signoff helpers
├── configs/                  # generated schemas, canonical examples, provider catalogs
├── docs/                     # architecture, config, testing, tool catalog, skills, PRDs
├── scripts/                  # lane runner, stress harness, static gates, coverage ratchet
└── .agent-harness/           # runtime-discovered agent profiles and skills
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| CLI behavior | `crates/harness/src/lib.rs`, `crates/harness/src/*.rs` | `main.rs` is a thin shim; in-process CLI tests use `CliIo`/`CliDeps`. |
| Runtime invariants | `crates/harness-core/AGENTS.md` | Read before changing events, coordinator, permissions, config, replay, lineage. |
| Provider protocol | `crates/harness-providers/AGENTS.md` | Read before changing `ProviderStreamEvent`, request metadata, cassettes, transports. |
| Native tools | `crates/harness-tools/AGENTS.md` | Read before changing schemas, path safety, bash, MCP, LSP, task/session/team tools. |
| TUI shell | `crates/harness-tui/AGENTS.md` | Read before touching transcript layout, app state, overlays, keybindings, snapshots. |
| E2E signoff tests | `crates/harness-testkit/tests/AGENTS.md` | PTY, live proxy, native visual, artifact provenance, env gates. |
| Runtime assets | `.agent-harness/AGENTS.md` | Agent profile markdown and skill packages loaded by the runtime. |
| Public config | `docs/config.md`, `configs/*.json`, `configs/*.jsonc` | Generated schemas are source of truth; examples and README must agree. |
| Test lanes | `docs/testing.md`, `scripts/test-lanes.sh` | Lane runner writes evidence artifacts; use the narrowest lane that proves the change. |

## CODE MAP
| Area | Role | High-value tests |
|------|------|------------------|
| `harness-core::coord` | Single scheduling/event/permission authority | `cargo test -p harness-core --test coord_test` |
| `harness-core::event` | Event schema v1 and append-only envelope | `cargo test -p harness --test event_docs_reference_test` |
| `harness-core::config` | Runtime/TUI config loading, public contract, schemas | `cargo test -p harness --test config_schema_cli_test` |
| `harness-tools` | Built-in native provider tool surface | `cargo test -p harness-tools --test native_tool_parity_matrix_test` |
| `harness-providers` | Streaming provider boundary and cassette replay | `cargo test -p harness-providers` |
| `harness-tui` | Presentation, view models, renderer, shell geometry | `cargo test -p harness-tui` |
| `harness-testkit` | Deterministic fakes, simulation, visual evidence | `cargo test -p harness-testkit --test simulation_validator_test` |

## FIRST-PARTY SEARCH SCOPE
- Include by default: `crates/`, `configs/`, `docs/`, `scripts/`, `.agent-harness/agents/`, `.agent-harness/skills/`, root manifests.
- Exclude by default: `target/`, `.git/`, `sessions/`, `artifacts/`, `.harness/`, `.gnhf/`, `.sisyphus/`, `.omx/`, `.codex/`, `inspirations/`.
- Search `inspirations/` only when explicitly comparing reference implementations.

## COMMANDS
```bash
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates
scripts/test-lanes.sh integration
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo run -p harness -- --config configs/harness.example.jsonc doctor
```
Targeted signoff:
```bash
scripts/test-lanes.sh simulation
scripts/test-lanes.sh signoff-binary
RUST_TEST_THREADS=1 cargo test -p harness-testkit --test pty_e2e
```

## CONVENTIONS
- Workspace lints deny `unsafe_code`, `dbg_macro`, and `todo`; clippy runs with `-D warnings`.
- Cargo workspace commands should be explicit: `--workspace` for all members, `-p <crate>` for scoped checks.
- `rust-toolchain.toml` pins stable plus `rustfmt` and `clippy`; CI also installs `cargo-nextest` and `cargo-llvm-cov`.
- Runtime config is `harness.json{,c}`; TUI config is `tui.json{,c}`. Keep them separate.
- Public permission names: `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp`.
- `task` calls require `prompt`, `run_in_background`, and `load_skills`; skills resolve before child spawn.
- `AGENTS.md` is project guidance; `.agent-harness/agents/*.md` are runtime profile assets. Do not mix those layers.

## MANDATORY CODING SKILLS
- For any coding work in this repository, load `karpathy-guidelines` before the first edit. Coding work includes implementation, bug fixes, refactors, tests, build scripts, schemas, and generated-code maintenance.
- Delegated coding tasks must include the skill in `load_skills`; omit it only for pure read-only exploration, documentation-only lookup, or non-coding QA.

## UPDATE TOGETHER
| Change | Also update |
|--------|-------------|
| Public config keys | `docs/config.md`, `configs/config.json`, `configs/tui.json`, config schema tests |
| Event variants or replay semantics | `docs/architecture.md`, event/replay docs tests |
| Native tool ids/schema/capability | `docs/native-tool-catalog.md`, `native_tool_parity_matrix_test` |
| Test lane behavior | `docs/testing.md`, `scripts/test-lanes.sh`, owner tests |
| Simulation invariants | `docs/simulation-matrix.json`, `simulation_validator_test`, secrets scan |
| Provider model catalog | `configs/provider-catalog.generated.json`, generated catalog docs/tests |
| Starter config defaults | `configs/harness.example.jsonc`, `configs/tui.example.jsonc`, README quick start |

## INVARIANTS
- Events are the source of truth; replay is side-effect free and derives from JSONL in `seq` order.
- Coordinator is the only event append, task scheduling, permission resolution, hook, and lifecycle authority.
- Permission checks precede tool execution; worker redelegation bypasses must remain blocked.
- Hashline edits validate anchors, reject overlaps, apply bottom-up, and write atomically.
- Provider-context compaction writes checkpoint artifacts/events; it must not rewrite `events.jsonl`.
- Session inspection tools read replay-derived data only; they must not execute providers, hooks, MCP, network, or the CLI.

## ANTI-PATTERNS
- Do not move runtime invariants into CLI, tools, providers, or TUI crates.
- Do not make replay execute tools, network calls, hooks, or provider work.
- Do not broaden compatibility aliases into the canonical public config contract.
- Do not treat runtime session/artifact directories as source.
- Do not claim PTY/live/native visual evidence without the matching lane or artifact provenance.

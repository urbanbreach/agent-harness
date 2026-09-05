# PROJECT KNOWLEDGE BASE

**Generated:** 2026-09-04T22:35:08.692Z
**Commit:** 4edb5153
**Branch:** dev

## OVERVIEW

Rust 2021 workspace for an agent harness: a coordinator-centered runtime with CLI,
provider, native-tool, terminal UI, and deterministic test-support crates.

## STRUCTURE

```text
agent-harness/
├── crates/
│   ├── harness/             # CLI adapter and command orchestration
│   ├── harness-core/        # coordinator, durable events, config, projections
│   ├── harness-providers/   # provider transports and stream normalization
│   ├── harness-tools/       # native and MCP tool registry/execution
│   ├── harness-tui/         # Ratatui/Crossterm live, replay, and review shells
│   └── harness-testkit/     # deterministic fakes and simulation support
├── configs/                 # strict JSON/JSONC configuration contracts
├── docs/                    # operator, architecture, and testing documentation
└── scripts/                 # test lanes, suite gates, and QA dogfood tooling
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Change runtime coordination | `crates/harness-core/src/coord/` | Coordinator owns transitions and authority |
| Change durable history | `crates/harness-core/src/event/`, `store/`, `session/`, `proj/` | Append-only events feed replay projections |
| Add or modify a CLI command | `crates/harness/src/` | `lib.rs` owns Clap routing; `main.rs` only calls `run_os()` |
| Add a provider/backend | `crates/harness-providers/src/` | Normalize backend protocol into common stream events |
| Add or modify tools | `crates/harness-tools/src/` | Registry, validation, execution, edit, LSP, and MCP boundaries |
| Change terminal behavior | `crates/harness-tui/src/` | Runtime I/O, state, view model, rendering, and terminal adapters |
| Add deterministic fixtures | `crates/harness-testkit/src/` | Fakes, workspaces, secret scanning, and simulation summaries |
| Run scoped test suites | `scripts/test-lanes.sh` | Canonical lane runner; gated modes fail closed |

## CODE MAP

Reference centrality was not measured; `Refs` records only that limitation.

| Symbol | Type | Location | Refs | Role |
|--------|------|----------|------|------|
| `CoordinatorHandle` / `spawn_coordinator` | struct / function | `crates/harness-core/src/coord/` | unmeasured | Async command API and runtime integration hub |
| `EventEnvelopeV1` / `EventV1` | types | `crates/harness-core/src/event/` | unmeasured | Versioned durable history schema |
| `HarnessConfig` | struct | `crates/harness-core/src/config/` | unmeasured | Runtime configuration hub |
| `run` / `run_os` | functions | `crates/harness/src/lib.rs` | unmeasured | In-process and operating-system CLI entry points |
| `Provider` / `ProviderStreamEvent` | trait / enum | `crates/harness-providers/src/lib.rs` | unmeasured | Backend contract and normalized stream vocabulary |
| `coordinator_registry_with_mcp_editing_and_executors` | function | `crates/harness-tools/src/lib.rs` | unmeasured | Native registry plus configured MCP tools |
| `AppState` / `render_app` | struct / function | `crates/harness-tui/src/app.rs`, `ui.rs` | unmeasured | UI state aggregate and pure frame composition |
| `run_tui_with_options` | function | `crates/harness-tui/src/runtime.rs` | unmeasured | Public terminal runtime entry |
| `build_normalized_summary` | function | `crates/harness-testkit/src/simulation.rs` | unmeasured | Deterministic simulation output |

## CONVENTIONS

- Runtime authority stays in the coordinator; leaf crates submit intents rather than
  appending events or independently owning permissions, scheduling, or lifecycle.
- Durable append-only events are authoritative for replay, session inspection, and
  projections. Replay and readiness paths remain side-effect-free and no-network.
- CLI paths use explicit `CliIo` and `CliDeps` seams and return integer status codes.
- Provider-specific streams are normalized before crossing the provider boundary.
- TUI rendering and view-model projection are pure; terminal I/O belongs to runtime
  and terminal adapters. Geometry uses grapheme/display-cell measurements.
- Integration tests run in process where possible; large targets aggregate numbered
  files with `include!`, and opt-in PTY/live/native evidence remains deterministic.
- Workspace lint policy denies unsafe code, unused must-use values, non-ASCII
  identifiers, unwrap/expect/panic/todo, and selected sharp Clippy patterns.

## ANTI-PATTERNS (THIS PROJECT)

- Do not replay historical tools or hooks, mutate source histories, or perform network
  work while inspecting replay/readiness state.
- Do not persist provider deltas, raw payloads, secrets, unredacted arguments, or
  provider reasoning details as durable state or support evidence.
- Do not bypass coordinator-owned permission, cancellation, scheduling, event-append,
  or lifecycle gates. Permissions are policy checks, not an OS sandbox.
- Do not treat unknown model limits or unavailable platform probes as success; preserve
  conservative unknown or structured unavailable outcomes.
- Do not let rendering mutate application state or allow lower-priority layers to
  consume input owned by an overlay.
- Do not create startup probe artifacts such as `harness.json`, `.agent-harness/plans`,
  `.harness-cow-probe`, `.harness-sessions-probe`, `.harness-foreign-probe-root`, or `.jj`.

## UNIQUE STYLES

- Support export scans fail closed: values are redacted, reasoning deltas removed, and
  no output is written after a secret finding.
- Terminal setup and teardown are capability-checked; unsafe links, controls, raw tool
  JSON, secrets, and sensitive paths are sanitized or rejected.
- Test lanes separate deterministic, integration, simulation, coverage, performance,
  PTY, native, live, and stress evidence; environment-gated lanes fail closed.

## COMMANDS

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --profile ci --workspace --all-features
scripts/test-lanes.sh quality-gates
scripts/test-lanes.sh fast
scripts/test-lanes.sh integration
scripts/test-lanes.sh all-deterministic
python3 scripts/check-test-suite-gates.py
bash scripts/harness-qa-dogfood.sh --self-test
```

## NOTES

- `rust-toolchain.toml` selects stable Rust with rustfmt and clippy; Cargo resolver 2
  coordinates the six-crate workspace.
- Nextest defaults to no retries and CPU-count parallelism, excludes performance,
  live, PTY, and native binaries, and serializes process-global-state tests.
- Performance contracts use release-mode tests; Linux PTY signoff requires
  `HARNESS_TUI_PTY_SIGNOFF=1`.

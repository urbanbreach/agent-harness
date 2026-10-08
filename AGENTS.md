# PROJECT KNOWLEDGE BASE

Backend map updated for the rewrite on 2026-09-27. Terminal ownership is unchanged.

## OVERVIEW

Rust 2024 workspace for an agent harness: a coordinator-centered runtime with CLI,
provider, native-tool, terminal UI, and deterministic test-support crates.

## SKILL USAGE

- Always use the Ponytail skill on ultra setting for all code work
- Always use the unslop skill for all text and prose work

## STRUCTURE

```text
agent-harness/
├── .github/workflows/       # GitHub Actions CI (Linux runners)
├── crates/
│   ├── harness/             # CLI adapter and command orchestration
│   ├── harness-core/        # coordinator, durable events, config, projections
│   ├── harness-providers/   # provider transports and stream normalization
│   ├── harness-eval/        # persistent code kernels and host-tool composition
│   ├── harness-tools/       # native and MCP tool registry/execution
│   ├── harness-tui/         # Ratatui/Crossterm live, replay, and review shells
│   └── harness-testkit/     # temporary workspaces and preserved terminal fixtures
├── configs/                 # strict JSON/JSONC configuration contracts
├── docs/                    # operator, architecture, and testing documentation
└── scripts/                 # test lanes, suite gates, and QA dogfood tooling
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Change runtime coordination | `crates/harness-core/src/coord/` | Coordinator owns transitions and authority |
| Change durable history | `crates/harness-core/src/event/`, `store.rs`, `session/`, `proj/` | Append-only events feed replay projections |
| Add or modify a CLI command | `crates/harness/src/` | `lib.rs` owns Clap routing; `main.rs` only calls `run_os()` |
| Add a provider/backend | `crates/harness-providers/src/` | Normalize backend protocol into common stream events |
| Add or modify tools | `crates/harness-tools/src/` | Registry, validation, execution, edit, LSP, and MCP boundaries |
| Change terminal behavior | `crates/harness-tui/src/` | Runtime I/O, state, view model, rendering, and terminal adapters |
| Add deterministic fixtures | The owning crate's `tests/` directory | Use existing local provider and tool fixtures; shared temporary workspaces live in `harness-testkit` |
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
| `coordinator_registry_with_skills` | function | `crates/harness-tools/src/lib.rs` | unmeasured | Native tool registry; runtime configuration adds MCP tools |
| `AppState` / `render_app` | struct / function | `crates/harness-tui/src/app.rs`, `ui.rs` | unmeasured | UI state aggregate and pure frame composition |
| `run_tui_with_options` | function | `crates/harness-tui/src/runtime.rs` | unmeasured | Public terminal runtime entry |
| `TestWorkspace` | struct | `crates/harness-testkit/src/workspace.rs` | unmeasured | Isolated temporary test workspace |

## CONVENTIONS

- Runtime authority stays in the coordinator; leaf crates submit intents rather than
  appending events or independently owning permissions, scheduling, or lifecycle.
- Durable append-only events are authoritative for replay, session inspection, and
  projections. Replay and readiness paths remain side-effect-free and no-network.
- CLI paths use explicit `CliIo` and `CliDeps` seams and return integer status codes.
- Provider-specific streams are normalized before crossing the provider boundary.
- TUI rendering and view-model projection are pure; terminal I/O belongs to runtime
  and terminal adapters. Geometry uses grapheme/display-cell measurements.
- Integration tests run in process where possible. Backend tests cover behavior at
  public boundaries; opt-in PTY/live/native evidence remains deterministic.
- Workspace lint policy denies unsafe code, unused must-use values, non-ASCII
  identifiers, unwrap/expect/panic/todo, and selected sharp Clippy patterns.
- Runtime data lives outside the project, resolved by `harness_core::storage_paths`:
  sessions in `<data>/sessions/<key>/`, memory, code index, edit attribution and plans
  in `<data>/projects/<key>/`, worktrees in `<data>/worktrees/<key>/`. `<data>` is
  `$HARNESS_DATA_HOME/harness`, `$XDG_DATA_HOME/harness`, or `~/.local/share/harness`;
  `<key>` wraps the canonical project path in `--`, with `/`, `\` and `:` turned into
  `-`. The project's `.agent-harness/` keeps only authored agents, skills, prompts and
  remembered permission grants. Library code takes injected paths; tests use temporary
  data roots.

## Tests

Tests are maintained code, not free safety.

Add the minimum test coverage necessary to protect meaningful behavior.

Do not add a test merely because production code changed.

Do not test:
- trivial getters/setters
- constructors with no meaningful behavior
- compiler-enforced type properties
- derived implementations
- straightforward delegation
- private implementation details already exercised through public behavior
- the same behavior repeatedly with different literal inputs
- impossible internal states solely to increase coverage

Before adding a test:
1. search for existing coverage;
2. prefer extending an existing test;
3. identify the specific plausible regression the new test prevents.

Prefer:
- one behavioral test over several implementation-detail tests;
- table-driven cases over repeated test functions;
- testing through the public boundary over private helpers.

A test that cannot plausibly catch a regression should not exist.

When running tests, use nextest instead of cargo test.

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
- Do not create startup probe artifacts such as `harness.json`, plan files inside the
  project, `.harness-cow-probe`, `.harness-sessions-probe`, `.harness-foreign-probe-root`,
  or `.jj`.

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
HARNESS_BINARY_SIGNOFF=1 cargo nextest run --profile ci -p harness --test binary_smoke --ignore-default-filter
bash scripts/harness-qa-dogfood.sh --self-test
```

## NOTES

- `rust-toolchain.toml` selects stable Rust with rustfmt and clippy; Cargo resolver 3
  coordinates the seven-crate workspace.
- Nextest defaults to no retries and CPU-count parallelism, excludes performance,
  live, PTY, and native binaries, and serializes process-global-state tests.
- Performance contracts use release-mode tests; Linux PTY signoff requires
  `HARNESS_TUI_PTY_SIGNOFF=1`.
- Harness supports Linux only; CI runs on GitHub Actions Ubuntu runners
  (`.github/workflows/ci.yml`). The performance workflow runs weekly or on demand.
- Default builds use the stock Rust linker. Wild is opt-in via
  `cargo --config .cargo/wild.toml`; see `docs/testing/build-performance.md`.
- Releases: add user-facing changes under `## [Unreleased]` in `CHANGELOG.md`;
  `scripts/release.sh <version>` stamps it, tags `v<version>` and pushes. The tag runs
  `.github/workflows/release.yml` (full CI, static musl x86_64/aarch64 binaries via
  cargo-zigbuild, mimalloc on musl, GitHub Release). See `docs/operations/releasing.md`.

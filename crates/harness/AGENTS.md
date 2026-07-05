# AGENTS: crates/harness

## OVERVIEW
CLI binary/library crate for command dispatch, config/bootstrap plumbing, auth, doctor/model readiness, prompt/run/replay/sessions commands, scripted scenarios, recovery stories, and the CLI-to-TUI handoff.

Read root `AGENTS.md` first. Runtime invariants belong in `harness-core`; this crate should stay an in-process testable shell around core, providers, tools, and the TUI crate.

## STRUCTURE
```text
src/
├── main.rs            # thin `harness::run_os()` binary shim
├── lib.rs             # Clap surface, command dispatch, `CliIo`, `CliDeps`
├── bootstrap.rs       # runtime catalog/profile/provider assembly for commands
├── cli_config.rs      # config path/discovery glue for CLI entrypoints
├── cli_io.rs          # injectable IO/env/cwd/process seams for tests
├── cli_labels.rs, defaults.rs, logging.rs, readiness.rs # shared CLI labels/defaults/logging/readiness helpers
├── auth_cmd/          # login/logout/list command flows and prompt UI helpers
├── prompt.rs, prompt/ # prompt command and streaming output helpers
├── run.rs             # run command coordinator/provider/tool wiring
├── replay.rs, replay/ # replay CLI output and recovery story rendering
├── sessions.rs, sessions/ # list/reopen/fork/export/session lineage surfaces (`export`, `lineage`, `list`)
├── tui.rs, tui/       # CLI-side TUI launch, live/replay/session history, lineage, workflow, auth backend
├── doctor*.rs         # readiness projection, metadata, doctor command
├── models.rs, model_probe.rs, runtime_catalog.rs, generated_model_catalog.rs
└── scenarios.rs       # deterministic built-in run scenarios
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Command routing | `src/lib.rs` | `Args`, `Command`, config resolution, top-level `execute_cli`. |
| Test injection | `src/cli_io.rs`, `src/lib.rs` | `CliIo` and `CliDeps` keep command tests in-process. |
| Bootstrap/catalog | `src/bootstrap.rs`, `src/runtime_catalog.rs`, `src/generated_model_catalog.rs` | Provider/model/profile assembly before core runtime starts. |
| Config/auth/models | `src/cli_config.rs`, `src/auth_cmd/`, `src/models.rs`, `src/model_probe.rs`, `src/doctor*.rs` | User-facing behavior; canonical config contract stays in core/docs/configs. |
| Run/prompt path | `src/run.rs`, `src/prompt.rs`, `src/prompt/`, `src/dynamic_prompt.rs`, `src/scenarios.rs` | Provider/coordinator setup and prompt asset composition. |
| TUI handoff | `src/tui.rs`, `src/tui/` | CLI config/profile/model/session metadata passed into `harness-tui`; rendering stays in `harness-tui`. |
| Replay/sessions | `src/replay.rs`, `src/replay/`, `src/sessions.rs`, `src/sessions/`, `src/recovery.rs` | Read replay-derived data only; do not execute live provider/tool work. |
| CLI tests | `tests/common/`, `tests/*_test.rs`, `tests/*/`, `tests/snapshots/` | Prefer fixture modules and in-process deps; shell out only when binary behavior is the point. |

## CLI RULES
- Keep `src/main.rs` thin; add command behavior to library modules so tests can call `run(...)`.
- Prefer `CliDeps`/`CliIo` injection for cwd, env, clock, provider, command runner, and output behavior.
- Bare `harness` launches the TUI path; root interactive flags should remain tied to that path unless CLI tests prove otherwise.
- Runtime/TUI config loading should go through existing config helpers; do not invent per-command discovery.
- Auth and model readiness commands may inspect local config/stored credentials but must not print secret values.
- Session/replay commands must stay inspection/export surfaces, not hidden execution entrypoints.
- Root `AGENTS.md` is included in composed prompt snapshots; update prompt snapshots only after reviewing intentional instruction drift.

## TESTS
```bash
cargo nextest run -p harness
cargo nextest run -p harness --test bootstrap_profiles_test
cargo nextest run -p harness --test config_schema_cli_test
cargo nextest run -p harness --test config_docs_reference_test
cargo nextest run -p harness --test prompt_cli_test
cargo nextest run -p harness --test replay_sessions_cli_test
cargo nextest run -p harness --test run_cli_test
cargo nextest run -p harness --test tui_cli_test
cargo nextest run -p harness --test binary_smoke -- --ignored
```

Use `HARNESS_UPDATE_PROMPT_SNAPSHOTS=1 cargo nextest run -p harness --test bootstrap_profiles_test` only when prompt/AGENTS/runtime-asset drift is intentional, then inspect the updated snapshots before keeping them.

## ANTI-PATTERNS
- Do not move coordinator, permission, event append, compaction, or replay semantics into the CLI.
- Do not bypass `CliIo`/`CliDeps` with process-global state in tests.
- Do not make config compatibility aliases canonical in help text, examples, or generated docs.
- Do not make replay/session commands call providers, tools, hooks, MCP, or network.
- Do not treat `src/tui/` as TUI rendering ownership; rendering lives in `crates/harness-tui`.

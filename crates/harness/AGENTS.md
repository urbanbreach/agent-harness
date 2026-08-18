# AGENTS: crates/harness

## OVERVIEW
CLI binary/library crate: Clap surface for 29 command variants, config/bootstrap plumbing, auth, doctor/model readiness, prompt/run, replay/sessions, leaf command domains, and the CLI-to-TUI handoff. Runtime invariants belong in `harness-core`; this crate is the in-process testable shell that owns command routing and output.

## STRUCTURE
```text
src/
├── main.rs          # thin shim: `harness::run_os()` binary entry
├── lib.rs           # `Cli`, `Commands` (29 variants), `run_os`, `run`, `execute_cli`, inline handlers
├── cli_io.rs        # `CliIo`: injectable stdin/stdout/stderr/cwd seams
├── bootstrap.rs     # runtime catalog/profile/provider assembly before core start
├── cli_config.rs    # config path/discovery glue for CLI entrypoints
├── cli_labels.rs, defaults.rs, logging.rs, readiness.rs   # shared CLI helpers
├── auth_cmd/        # login/logout/list, prompt UI, sleep/wake, auth backend args
├── prompt.rs, prompt/    # prompt command + streaming output helpers
├── run.rs, dynamic_prompt.rs, scenarios.rs   # run wiring, prompt composition, built-in scenarios
├── replay.rs, replay/    # replay output + recovery story rendering
├── sessions.rs, sessions/  # list/reopen/fork/export/lineage/rewind surfaces
├── tui.rs, tui/      # CLI-side TUI launch; rendering lives in `harness-tui`
├── doctor.rs, doctor/, doctor_metadata.rs   # readiness projection, checks, doctor command
├── models.rs, model_probe.rs, runtime_catalog.rs, generated_model_catalog.rs
└── *_cmd.rs         # leaf command modules (see WHERE TO LOOK)
```

## WHERE TO LOOK
| Task | Location |
|------|----------|
| Binary entry | `src/main.rs` -> `run_os()` in `src/lib.rs` |
| Dispatch | `execute_cli` in `src/lib.rs`; `Cli`/`Commands` enum + per-command arg structs |
| Test seams | `CliIo` in `src/cli_io.rs`, `CliDeps` in `src/lib.rs` |
| Auth | `src/auth_cmd/` |
| Prompt/run | `src/prompt.rs`, `src/prompt/`, `src/run.rs`, `src/dynamic_prompt.rs`, `src/scenarios.rs` |
| Replay/sessions | `src/replay.rs`, `src/replay/`, `src/sessions.rs`, `src/sessions/`, `src/recovery.rs` |
| TUI handoff | `src/tui.rs`, `src/tui/` |
| Doctor/models | `src/doctor.rs`, `src/doctor/`, `src/doctor_metadata.rs`, `src/models.rs`, `src/model_probe.rs`, `src/runtime_catalog.rs`, `src/generated_model_catalog.rs` |
| Leaf commands | `src/memory_cmd.rs` (memory), `src/worktree_cmd.rs`, `src/attribution_cmd.rs`, `src/prompt_queue_cmd.rs`, `src/cron_cmd.rs`, `src/team_cmd.rs`, `src/plugin_cmd.rs`, `src/update_cmd.rs`, `src/providers_cmd.rs`, `src/code_graph_cmd.rs`, `src/dashboard_cmd.rs`, `src/agent_stdio_cmd.rs` (agent) |
| Inline handlers | `Schema`, `Config`, `Completions`, `Export`, `Trace`, `Share`, `Setup`, `Wrap`, `Mcp` handled in `execute_cli` with arg structs and `execute_*` fns in `src/lib.rs` |
| Shared plumbing | `src/bootstrap.rs`, `src/cli_config.rs`, `src/cli_labels.rs`, `src/defaults.rs`, `src/logging.rs`, `src/readiness.rs` |

## CLI RULES
- Keep `src/main.rs` a shim; put command behavior in library modules so tests call `run(...)`.
- Prefer `CliDeps`/`CliIo` injection for cwd, env, clock, provider, command runner, and output.
- Define command arg structs where their handler lives: leaf domains own theirs in `*_cmd.rs`; inline commands keep theirs in `src/lib.rs`.
- Bare `harness` launches the TUI path via `tui::execute_with_io`; root interactive flags stay tied to that path.
- Route config loading through `cli_config.rs` helpers; do not invent per-command discovery.
- Auth/doctor may inspect local config/credentials but must not print secret values.
- Session/replay commands stay inspection/export surfaces, not hidden execution entrypoints.
- Root `AGENTS.md` is included in composed prompt snapshots; update prompt snapshots only after reviewing intentional instruction drift.

## TESTS
Owner suites live in `crates/harness/tests/`; read `tests/AGENTS.md` before editing them. Quick entry:
```bash
cargo nextest run -p harness
cargo nextest run -p harness --test config_schema_cli_test
cargo nextest run -p harness --test prompt_cli_test
cargo nextest run -p harness --test replay_sessions_cli_test
cargo nextest run -p harness --test tui_cli_test
```

## ANTI-PATTERNS
- Do not move coordinator, permission, event append, compaction, or replay semantics into the CLI.
- Do not bypass `CliIo`/`CliDeps` with process-global state in tests.
- Do not make config compatibility aliases canonical in help text, examples, or generated docs.
- Do not treat `src/tui/` as TUI rendering ownership; rendering lives in `crates/harness-tui`.
- Do not add command variants without a dispatch arm in `execute_cli` and an owner suite in `tests/`.

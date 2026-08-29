# AGENTS: crates/harness/tests

## OVERVIEW
Owner integration suites for the CLI crate. Cover command routing, heavy domains (config/auth/prompt/run/replay/sessions/TUI), leaf command owners, drift guards, and gated binary/PTY evidence. Read `crates/harness/AGENTS.md` for command ownership; this file covers test-scope rules only.

## STRUCTURE
```text
tests/
├── common/                 # shared fixtures and helpers (see WHERE TO LOOK)
├── snapshots/              # prompt-asset snapshots (v1_* dirs + v1_prompt_assets.json)
├── *_cli_test.rs           # numbered-suite aggregators (`include!` per part)
├── config_schema_cli/      # 01..07: models/doctor/config/auth/worktree/settings
├── prompt_cli/             # 01..04: prompt paths, model variant, profile routing, tools
├── replay_sessions_cli/    # 01..13 + 08b: replay/sessions export/fork/reopen surfaces
├── tui_cli/                # 01..03: TUI help/launch/bootstrap behavior
├── bootstrap_profiles/     # runtime bootstrap + permission rule export
└── config_docs_reference/  # docs contract surface
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| In-process CLI runs | `common/cli_harness.rs` | `CliHarness` drives `harness::run(...)` with injected `CliIo`/`CliDeps`; captures events/artifacts/workspace. |
| Fixtures/helpers | `common/` | Per-suite fixture modules (`config_schema_cli_fixtures`, `prompt_cli_fixtures`, `replay_sessions_cli_fixtures`, `tui_cli_fixtures`, `bootstrap_profile_helpers`), `repo_root.rs`. |
| Numbered suite aggregators | `config_schema_cli_test.rs`, `prompt_cli_test.rs`, `replay_sessions_cli_test.rs`, `tui_cli_test.rs`, `bootstrap_profiles_test.rs`, `config_docs_reference_test.rs` | Each binary `include!`s the matching numbered part files under its directory. |
| Leaf command owners | `run_cli_test.rs`, `worktree_cli_test.rs`, `attribution_cli_test.rs`, `dashboard_cli_test.rs`, `agent_stdio_cli_test.rs`, `cron_concurrency_test.rs`, `support_export_test.rs` | One suite per leaf `*_cmd` module; mostly in-process. |
| Read-only surface guards | `session_inspect_side_effect_free_test.rs`, `tui_replay_read_only_test.rs`, `determinism_multi_turn_tools_test.rs` | Prove inspection never executes providers/tools/network. |
| Drift/contract guards | `cli_contract_matrix_test.rs`, `cli_authority_matrix_cli_test.rs`, `config_docs_reference_contract_test.rs`, `event_docs_reference_test.rs`, `integrations_matrix_test.rs` | Pin retained CLI surface, authority, and documentation contracts; update only when the contract intentionally changes. |
| Security guards | `poc_candidate2_auth_secret_leakage_test.rs` | Secrets never print. |
| Binary/PTY owners | `binary_smoke.rs`, `pty_happy_path_recorded.rs` | `#[ignore]`d signoff lanes; run via `scripts/test-lanes.sh`, never default nextest. |

## TEST RULES
- Prefer in-process `CliIo`/`CliDeps`; shell out only when binary behavior is the point (`binary_smoke.rs`, PTY owners).
- Numbered part files are `include!`d by aggregator binaries; keep shared setup in `common/`, do not duplicate fixtures.
- Never bypass `CliIo`/`CliDeps` with process-global state; replay-derived suites must not execute providers, tools, hooks, MCP, or network.
- Drift guards pin the retained surface; changing behavior without updating the guard's contract is a failure.
- Run the full crate suite before editing unrelated owners: `cargo nextest run -p harness`.
- Use `HARNESS_UPDATE_PROMPT_SNAPSHOTS=1` only for intentional prompt/AGENTS/runtime-asset drift, then inspect updated snapshots before keeping them.

## COMMANDS
```bash
cargo nextest run -p harness
scripts/test-lanes.sh signoff-binary
scripts/test-lanes.sh signoff-pty
```

## ANTI-PATTERNS
- Do not assert snapshot prose; assert routing/token-level contracts instead.
- Do not add fixtures that duplicate `common/` or create fixture sprawl inside a numbered part file.
- Do not add speculative owners for commands without live handlers; every owner suite must trace to a dispatched command.
- Do not claim PTY/live/native evidence without the matching lane and artifact provenance.

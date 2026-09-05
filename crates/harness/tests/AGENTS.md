# CLI TEST GUIDE

## OVERVIEW

Integration, contract, snapshot, PTY, and lane-policy coverage for the CLI; score 11 from 97 files, nine subdirectories, high Rust ratio, and measured symbol/export density.

## STRUCTURE

```text
tests/
├── common/                 # in-process CLI harness and shared fixtures
├── config_schema_cli/      # schema, doctor, auth, discovery contracts
├── replay_sessions_cli/    # replay, history, lineage, export contracts
├── prompt_cli/             # endpoint, model, profile, and tool-loop cases
├── cli_contract_matrix/    # command authority and removed-leaf audits
├── bootstrap_profiles/     # profile/tool/permission construction
├── tui_cli/                # CLI-side TUI startup and help contracts
└── snapshots/              # shipped prompt assets and composed output
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Drive commands in-process | `common/cli_harness.rs` | Args, stdin, env, provider, session capture |
| Build prompt fixtures | `common/prompt_cli_fixtures.rs` | Scripted providers and event assertions |
| Build replay fixtures | `common/replay_sessions_cli_fixtures.rs` | JSONL histories and session layouts |
| Audit public CLI | `cli_contract_matrix_test.rs` | Includes focused matrix parts |
| Validate config/doctor | `config_schema_cli_test.rs` | Aggregates numbered owner files |
| Validate sessions/export | `replay_sessions_cli_test.rs` | Aggregates numbered contract files |
| Exercise real terminal paths | `pty_happy_path_recorded.rs`, `binary_smoke.rs` | Ignored signoff surfaces |

## CONVENTIONS

- Prefer `CliHarness` or direct `harness::run` with buffered `CliIo`; spawn the binary only when process or terminal behavior is the contract.
- Aggregator targets use `include!` to share fixtures and partition large suites into numbered owner files; run the aggregator target, not a part file.
- Arrange filesystem state in `tempdir` or `TestWorkspace`, inject provider and environment behavior, then assert status plus the correct output stream.
- Assert structured JSON fields and durable events instead of prose; plain-text assertions are reserved for intentional human CLI contracts.
- Subscribe to the exact event or channel before triggering async work, then await it with a bounded timeout; never add fixed sleeps.
- Deterministic cases use stable seeds, clocks, run IDs, and scripted providers; network/live/native coverage stays opt-in.

## ANTI-PATTERNS

- Do not mutate process-global cwd or environment in ordinary tests; use `CliDeps`/`CliHarness` overrides.
- Do not add real-world dependencies outside files ending `_recorded.rs`, and do not move opt-in signoff cases into default lanes.
- Do not update prompt snapshots unless the shipped prompt contract intentionally changed and `HARNESS_UPDATE_PROMPT_SNAPSHOTS=1` is explicit.
- Do not mask isolation defects with serial execution groups, retries, timing waits, or broad mocks that bypass the integration under test.
- Do not assert secret literals in failure output; verify redaction markers and absence across stdout, stderr, events, and artifacts.
- Do not run numbered included files as standalone Cargo targets; target their top-level `*_test.rs` owner.

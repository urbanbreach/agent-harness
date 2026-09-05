# HARNESS CLI KNOWLEDGE BASE

## OVERVIEW

Rust CLI entrypoint and operator-facing adapter for the event-sourced runtime; score 12, selected as a distinct crate with high file, code, symbol, export, config, and module-boundary density.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Command grammar and dispatch | `src/lib.rs` | Clap root, dependency seams, exit handling |
| Binary entry | `src/main.rs` | Thin `harness::run_os()` shim |
| Headless execution | `src/run.rs`, `src/prompt.rs` | Runtime setup and prompt lifecycle |
| Interactive execution | `src/tui.rs`, `src/tui/` | Live, replay, and continuation workflows |
| Session operations | `src/sessions.rs`, `src/replay.rs` | Catalog, history, lineage, export, recovery |
| Auth and model setup | `src/auth_cmd.rs`, `src/models.rs` | Credential status and catalog surfaces |
| Integration contracts | `tests/` | In-process, binary, PTY, and snapshot coverage |

## CONVENTIONS

- Keep `main.rs` inert; callable behavior belongs in the library so tests can supply `CliIo` and `CliDeps`.
- Clap derive types define public grammar; command handlers write through supplied streams and return integer exit codes.
- Success is `0`, operational failure is `1`, and invalid or intentionally unsupported usage is generally `2`.
- Human and JSON output are parallel operator contracts; machine output uses typed serializable views rather than parsed prose.
- Durable state authority remains in `harness-core`; this crate resolves options, invokes domain stores, and translates errors.
- Configuration and credentials are injected through explicit paths, environment views, stores, clocks, providers, and runners.

## COMMANDS

```bash
cargo build -p harness
cargo nextest run -p harness
scripts/test-lanes.sh fast
scripts/test-lanes.sh integration
```

Use the narrowest relevant integration target while iterating; `scripts/test-lanes.sh` is the canonical signoff runner.

## ANTI-PATTERNS

- Never expose API keys, OAuth tokens, credential-store contents, secret-shaped settings, or unredacted support artifacts.
- Never make replay, session inspection, doctor, discovery, or worktree read paths invoke providers, tools, hooks, or network probes.
- Do not restore deprecated leaves as fake successes; unsupported compatibility surfaces intentionally fail with actionable output.
- Do not bypass append-only event ownership by rewriting `events.jsonl`; derive read views from canonical projections.
- Do not invent model capabilities, session state, or extension execution support when source data is unknown or descriptor-only.
- Preserve unrelated workspace edits and generated fixture contracts; snapshot updates require their explicit opt-in environment switch.

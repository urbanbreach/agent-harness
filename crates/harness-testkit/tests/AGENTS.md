# AGENTS: crates/harness-testkit/tests

## OVERVIEW
Test-orchestration subtree for deterministic PTY E2E and env-gated live-proxy signoff. This is workflow-heavy test code, not reusable runtime logic.

## STRUCTURE
```text
tests/
├── pty_e2e.rs             # offline deterministic PTY lane
├── live_proxy_e2e.rs      # real provider / real tool-flow lane
├── README.live-proxy.md   # preflight, env, artifact layout, retention
├── support/               # live_events, live_vision, live_visual helpers
└── snapshots/             # screenshot/snapshot expectations
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Offline UI regression lane | `pty_e2e.rs` | Single-threaded, deterministic, artifact-producing |
| Live proxy signoff lane | `live_proxy_e2e.rs` | Ignored/live; env-gated |
| Live lane setup and artifact contract | `README.live-proxy.md` | Preflight first |
| Shared live helpers | `support/live_events.rs`, `support/live_vision.rs`, `support/live_visual.rs` | Reuse before adding new helper files |

## CONVENTIONS
- Treat `pty_e2e` as the default offline lane; treat `live_proxy_*` as explicit signoff.
- Run PTY flows single-threaded.
- Respect `HARNESS_VISUAL_ARTIFACT_DIR` for screenshot output.
- Respect live env gates: `HARNESS_LIVE_PROXY`, `HARNESS_LIVE_PROXY_CONFIG`, `HARNESS_LIVE_PROXY_PROVIDER`, `HARNESS_LIVE_PROXY_MODEL`.
- Artifact retention and viewport presets are documented here, not in crate root docs.

## ANTI-PATTERNS
- Do not run live-proxy lanes without the documented preflight.
- Do not assume run IDs or screenshot artifact paths are stable across executions.
- Do not parallelize PTY tests or remove determinism env guards.

## COMMANDS
```bash
RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e
HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.3-codex cargo test -p harness-testkit live_proxy_preflight -- --ignored --exact
HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.3-codex HARNESS_VISUAL_ARTIFACT_DIR=target/live-proxy-visual-artifacts cargo test -p harness-testkit live_proxy_e2e_tui_tool_flow -- --ignored --exact
```

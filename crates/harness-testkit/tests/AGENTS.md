# AGENTS: crates/harness-testkit/tests

## OVERVIEW
Test-orchestration subtree for deterministic PTY E2E and env-gated live-proxy signoff. This is workflow-heavy test code, not reusable runtime logic.

Read the workspace root `AGENTS.md` first for crate ownership, search exclusions, and the cross-crate verification matrix.

## STRUCTURE
```text
tests/
├── pty_e2e.rs             # offline deterministic PTY lane
├── live_proxy_e2e.rs      # real provider / real prompt + TUI tool-flow lanes
├── README.live-proxy.md   # preflight, env, artifact layout, retention
├── support/               # live_events, live_vision, live_visual helpers
└── snapshots/             # screenshot/snapshot expectations
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Offline UI regression lane | `pty_e2e.rs` | Single-threaded, deterministic, artifact-producing |
| Live prompt chat-control lane | `live_proxy_e2e.rs` | Real model + live config for `todowrite`, `question`, and `skill` |
| Live proxy TUI tool-flow lane | `live_proxy_e2e.rs` | Real model + live config for file/tool-flow and screenshots |
| Live lane setup and artifact contract | `README.live-proxy.md` | Preflight first |
| Shared test helpers | `support/` | Reuse and extend helper modules before adding more test logic to `pty_e2e.rs` |

## RENDERING DEPENDENCIES
Approved rendering stack (do not add alternatives without explicit signoff):
- `syntect` for syntax highlighting
- `imara-diff` for diff visualization

## SHELL CONTRACT (T14+)
The TUI implements a strict surface hierarchy that tests must respect:
- **Compose-first home screen**: entry point is the composer, not a replay browser.
- **Transcript-first session shell**: live sessions prioritize transcript rendering with the operator sidebar for context/tooling.
- **Operator sidebar**: persistent right-hand surface for operator state, file context, and tool status.
- **No default tab chrome**: surfaces are chromeless by default; tab-like chrome is opt-in per context.
- **No debug inspector in the primary path**: debug/inspector surfaces live in secondary paths, not the main UX flow.

## CONVENTIONS
- Treat `pty_e2e` as the default offline lane; treat `live_proxy_*` as explicit signoff.
- Run PTY flows single-threaded.
- Prefer extracting markers, visual contracts, and repeated assertion/setup helpers into `support/` modules instead of growing `pty_e2e.rs`.
- **Screenshot-generated PTY/live-visual artifacts are the primary verification workflow**. Prefer visual parity over text assertions.
- For chat/tool-flow iteration, use the live-config order: `live_proxy_preflight` → `live_proxy_prompt_chat_tool_flow` → `live_proxy_e2e_tui_tool_flow`.
- Key shell contract terms: compose-first, transcript-first, operator sidebar, no default tab chrome, no debug inspector in primary path.
- Respect `HARNESS_VISUAL_ARTIFACT_DIR` for screenshot output. This env var sets the root for all visual artifacts in both PTY and live-proxy lanes.
- Respect live env gates: `HARNESS_LIVE_PROXY`, `HARNESS_LIVE_PROXY_CONFIG`, `HARNESS_LIVE_PROXY_PROVIDER`, `HARNESS_LIVE_PROXY_MODEL`.
- `live_proxy_preflight` is Linux-only because it validates the live TUI lane setup.
- `live_proxy_prompt_chat_tool_flow` depends on the `rust-best-practices` skill being available to the `skill` tool.
- Artifact retention and viewport presets are documented in `README.live-proxy.md`, not in crate root docs.

## ANTI-PATTERNS
- Do not run live-proxy lanes without the documented preflight.
- Do not assume run IDs or screenshot artifact paths are stable across executions.
- Do not parallelize PTY tests or remove determinism env guards.

## COMMANDS
```bash
RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e
HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini cargo test -p harness-testkit live_proxy_preflight -- --ignored --exact
HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini cargo test -p harness-testkit live_proxy_prompt_chat_tool_flow -- --ignored --exact
HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts cargo test -p harness-testkit live_proxy_e2e_tui_tool_flow -- --ignored --exact
```

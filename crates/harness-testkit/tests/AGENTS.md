# AGENTS: crates/harness-testkit/tests

## OVERVIEW
Workflow-heavy E2E tests for native screenshot signoff, deterministic PTY simulation, and env-gated live-proxy signoff. Runtime-independent helpers belong in `crates/harness-testkit/src/`.

Read the workspace root `AGENTS.md` first. TUI shell contract details live in `crates/harness-tui/AGENTS.md`; this file focuses on test lanes and artifact provenance.

## STRUCTURE
```text
tests/
├── native_visual_e2e.rs   # local Ghostty/tmux real-screenshot signoff lane
├── pty_e2e.rs             # offline deterministic PTY lane
├── live_proxy_e2e.rs      # env-gated live proxy smoke/preflight wrappers
├── README.live-proxy.md   # preflight, env, artifact layout, retention
├── support/               # small helpers retained by deterministic support tests
└── snapshots/             # PTY snapshot expectations
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Native local screenshot lane | `native_visual_e2e.rs` | Ghostty renderer + tmux control + manifest-backed screenshots. |
| Offline UI regression lane | `pty_e2e.rs` | Single-threaded, deterministic, artifact-producing fallback. |
| Live proxy smoke lanes | `live_proxy_e2e.rs` | Env/config preflight plus retained signoff entrypoint names; behavior assertions live in deterministic owners listed in `docs/testing.md`. |
| Live setup and artifacts | `README.live-proxy.md` | Preflight and retention contract. |
| Shared helpers | `support/` | Keep helpers small and only for tests that still import them. |

## LANE ORDER
- PTY fallback: `RUST_TEST_THREADS=1 cargo test -p harness-testkit --test pty_e2e`.
- Live TUI order: `live_proxy_preflight_requires_live_env` → `live_proxy_e2e_tui_parity_signoff`.
- Live CLI order: `live_proxy_preflight_requires_live_env` → `live_proxy_prompt_parity_signoff`.
- Native screenshots are local signoff only; run ignored native tests single-threaded.

## ENV CONTRACT
- Visual artifacts root: `HARNESS_VISUAL_ARTIFACT_DIR`.
- Native gates: `HARNESS_NATIVE_VISUAL`, `HARNESS_NATIVE_VISUAL_FONT_FAMILY`, `HARNESS_NATIVE_VISUAL_FONT_SIZE`, `HARNESS_NATIVE_VISUAL_CAPTURE_HELPER`.
- Live gates: `HARNESS_LIVE_PROXY`, `HARNESS_LIVE_PROXY_CONFIG`, `HARNESS_LIVE_PROXY_PROVIDER`, `HARNESS_LIVE_PROXY_MODEL`, optional `HARNESS_LIVE_PROXY_VARIANT`.

## CONVENTIONS
- Treat `pty_e2e` as the deterministic CI/headless oracle and fallback lane.
- Treat `live_proxy_*` as explicit env-gated signoff entrypoints; never run without documented preflight, and do not claim provider/tool-flow behavior unless a deterministic owner or a real live run supplied the evidence.
- Treat native screenshots as provenance-checked local visual signoff, not a portable hash oracle.
- Manifest-backed screenshots plus PTY/live artifacts are the verification record; do not claim success without artifact paths and capture provenance.
- The native lane assumes Ghostty + tmux + `xprop` plus an authorized capture helper inside managed 2560×1440 nested KWin/XWayland; if unavailable, fail closed and use PTY.
- When `HARNESS_NATIVE_VISUAL=1`, missing `DISPLAY` is a hard failure, not a skip.

## ANTI-PATTERNS
- Do not parallelize PTY/native visual lanes or remove determinism env guards.
- Do not assume run IDs or screenshot artifact paths are stable across executions.
- Do not edit renderer defaults, font candidate order, raster/cell sizing, viewport, focus regions, or capture settings unless the task is visual fidelity and you rerun the visual lane.
- Do not claim native screenshot success unless capture used the exact terminal window id and cleaned it up afterward.

## COMMANDS
```bash
RUST_TEST_THREADS=1 cargo test -p harness-testkit --test pty_e2e
HARNESS_NATIVE_VISUAL=1 cargo test -p harness-testkit --test native_visual_e2e -- --ignored --test-threads=1
HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini cargo test -p harness-testkit live_proxy_preflight_requires_live_env -- --ignored --exact
HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini cargo test -p harness-testkit live_proxy_prompt_parity_signoff -- --ignored --exact
HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts cargo test -p harness-testkit live_proxy_e2e_tui_parity_signoff -- --ignored --exact
```

# AGENTS: crates/harness-testkit/tests

## OVERVIEW
Workflow-heavy E2E tests for native screenshot signoff, deterministic PTY simulation, env-gated live-proxy signoff, simulation validation, and artifact provenance. Runtime-independent helpers belong in `crates/harness-testkit/src/`.

Read root `AGENTS.md` first. TUI shell contracts live in `crates/harness-tui/AGENTS.md`.

## STRUCTURE
```text
tests/
├── pty_e2e.rs             # offline deterministic PTY lane; custom test target
├── live_proxy_e2e.rs      # env-gated live proxy preflight/signoff wrappers
├── native_visual_e2e.rs   # local Ghostty/tmux screenshot signoff lane
├── simulation_validator_test.rs
├── secretscan_test.rs
├── focus_region_test.rs
├── README.live-proxy.md
├── support/
└── snapshots/
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Offline UI regression | `pty_e2e.rs` | Single-threaded deterministic PTY evidence. |
| Live proxy setup | `README.live-proxy.md`, `live_proxy_e2e.rs` | Env/config preflight; behavior assertions mostly live in deterministic owners. |
| Native screenshots | `native_visual_e2e.rs` | Ghostty renderer, tmux control, manifest-backed screenshots. |
| Simulation validation | `simulation_validator_test.rs` | Checks `docs/simulation-matrix.json`. |
| Secret hygiene | `secretscan_test.rs` | Scans simulation/cassette artifacts when env points at them. |
| Shared helpers | `support/` | Keep small and local to tests that import them. |

## LANE ORDER
- PTY fallback: `RUST_TEST_THREADS=1 cargo test -p harness-testkit --test pty_e2e`.
- Live CLI: `live_proxy_preflight_requires_live_env` -> `live_proxy_prompt_parity_signoff`.
- Live TUI: `live_proxy_preflight_requires_live_env` -> `live_proxy_e2e_tui_parity_signoff`.
- Native screenshots: ignored native tests, single-threaded, local signoff only.
- Simulation: run through `scripts/test-lanes.sh simulation` so replay/evidence/secret-scan stages stay coupled.

## ENV CONTRACT
- Live gates: `HARNESS_LIVE_PROXY`, `HARNESS_LIVE_PROXY_CONFIG`, `HARNESS_LIVE_PROXY_PROVIDER`, `HARNESS_LIVE_PROXY_MODEL`, optional `HARNESS_LIVE_PROXY_VARIANT`.
- Native gates: `HARNESS_NATIVE_VISUAL`, `DISPLAY`, optional font/capture helper variables.
- Visual artifact root: `HARNESS_VISUAL_ARTIFACT_DIR`.
- Simulation scan: `HARNESS_SECRETS_SCAN_ARTIFACTS`, `HARNESS_SIMULATION_ARTIFACT_DIR`.

## CONVENTIONS
- Treat PTY as the deterministic CI/headless oracle and fallback lane.
- Treat live proxy wrappers as explicit env-gated signoff names, not broad live behavioral coverage.
- Treat native screenshots as provenance-checked local visual signoff, not a portable hash oracle.
- Manifest-backed screenshots plus PTY/live artifacts are the verification record; report artifact paths when claiming success.
- When `HARNESS_NATIVE_VISUAL=1`, missing `DISPLAY` is a hard failure, not a skip.

## COMMANDS
```bash
RUST_TEST_THREADS=1 cargo test -p harness-testkit --test pty_e2e
HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini cargo test -p harness-testkit live_proxy_preflight_requires_live_env -- --ignored --exact
HARNESS_NATIVE_VISUAL=1 cargo test -p harness-testkit --test native_visual_e2e -- --ignored --test-threads=1
```

## ANTI-PATTERNS
- Do not parallelize PTY/native visual lanes or remove determinism env guards.
- Do not assume run ids or screenshot paths are stable across executions.
- Do not claim provider/tool-flow behavior from slim live wrappers alone.
- Do not edit renderer defaults or capture settings unless the task is visual fidelity and you rerun the matching lane.

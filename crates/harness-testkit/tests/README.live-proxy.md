# Live proxy E2E lane

This test lane proves real tool use against the configured live proxy/model.

## Preflight

Run this first when validating local setup:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.3-codex \
cargo test -p harness-testkit live_proxy_preflight -- --ignored --exact
```

The preflight verifies:

- live config path resolves
- provider/model/profile selection succeeds
- provider `api_mode` is `responses` or `auto`
- the harness binary is available
- the configured proxy host:port is reachable

`live_proxy_preflight` currently expects Linux because it validates the live TUI lane as well as
the provider wiring.

## Main live tool-flow test

Run the prompt-based chat-control lane first when the change is about tool orchestration, todo/question state, or agent workflow helpers:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.3-codex \
cargo test -p harness-testkit live_proxy_prompt_chat_tool_flow -- --ignored --exact
```

This lane exercises a real model against prepared live profiles for:

- `todowrite`
- `question`
- `skill`

The prompt chat-control lane assumes the `rust-best-practices` skill is installed in the configured
skill directories because the live provider is asked to load it through the `skill` tool.

Then run the file-edit / visual lane:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.3-codex \
HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts \
cargo test -p harness-testkit live_proxy_e2e_tui_tool_flow -- --ignored --exact
```

## Local helper trust checks

These exact helper tests keep the manifest/retention layer machine-verifiable without requiring
live proxy credentials:

```bash
cargo test -p harness-testkit live_visual_checkpoint_writes_png_and_manifest -- --exact
cargo test -p harness-testkit live_visual_run_retention_prunes_old_runs -- --exact
```

## Artifact layout

Artifacts are written under:

```text
<artifact-root>/live-proxy/<test-name>/<run-id>/
```

Recommended local root for both offline PTY and live manifest inspection:

```text
target/pty-visual-artifacts/
```

Offline PTY parity evidence now keeps the frozen PNGs at the artifact root and writes
family manifests under:

```text
target/pty-visual-artifacts/pty-manifests/<family>/manifest.json
target/pty-visual-artifacts/pty-manifests/<family>/manifest.jsonl
```

Those PTY manifests use the same `manifest.json` / `manifest.jsonl` filenames as the live proxy
lane so marker presence, focus hashes, and PNG paths stay machine-checkable across both oracles.

Example run id:

```text
run-20260307-081508-190134Z
```

Each run directory includes:

- `live_proxy_startup.png`
- `live_proxy_draft_visible.png`
- `live_proxy_shell_create_finished.png`
- `live_proxy_hashline_scan_finished.png`
- `live_proxy_run_finished.png`
- `manifest.json`
- `manifest.jsonl`
- `run_summary.json`
- `run_summary.txt`

## Retention

- default: keep the latest **5** screenshot runs per test
- override with `HARNESS_LIVE_VISUAL_KEEP_RUNS=<n>`
- pruning only applies to manifest-backed `run-*` evidence directories; sidecars stay untouched

## Viewport presets

The live screenshot viewport is configurable:

- `desktop` (default)
- `laptop`
- `compact`

Set with:

```bash
HARNESS_LIVE_VISUAL_VIEWPORT=desktop
```

## Notes for agents

Agents can run this lane if the required env vars are present and the local proxy is reachable.
Use this order while iterating:

1. `live_proxy_preflight`
2. `live_proxy_prompt_chat_tool_flow`
3. `live_proxy_e2e_tui_tool_flow`
4. `live_proxy_e2e_visual_verifier` only for screenshot/signoff work

The tests are still live-model dependent, so retries are expected and already built into the
TUI tool-flow lane.

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

## Main live tool-flow test

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.3-codex \
HARNESS_VISUAL_ARTIFACT_DIR=target/live-proxy-visual-artifacts \
cargo test -p harness-testkit live_proxy_e2e_tui_tool_flow -- --ignored --exact
```

## Artifact layout

Artifacts are written under:

```text
<artifact-root>/live-proxy/<test-name>/<run-id>/
```

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

Agents can run this lane if the required env vars are present and the local proxy is reachable. The tests are still live-model dependent, so retries are expected and already built into the tool-flow lane.

# Live proxy E2E lane

This test lane proves real tool use against the configured live proxy/model.

## Preflight

Run this first when validating local setup:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_preflight -- --ignored --exact
```

The preflight verifies:

- live config path resolves
- provider/model/profile selection succeeds
- provider `api_mode` is `responses` or `auto`
- the harness binary is available
- the configured proxy host:port is reachable
- the later live visual lane can use the bundled PTY→PNG capture path and the shell-free
  `fs.write` bootstrap used by the file-edit review flow

The live visual review lane does **not** require KDE, `konsole`, or `spectacle`. When you run the
later live TUI/tool-flow lanes, screenshots are rendered from captured PTY state into PNGs inside
the harness, and the tool-flow bootstrap uses `fs.write` instead of `shell.run`, so the signoff
path no longer depends on a desktop session or a local POSIX shell. `live_proxy_preflight` itself
only verifies config/provider reachability plus the prepared live-config path.

When the shipped `configs/harness.example.jsonc` is the active live config, the interactive
`build` profile now defaults `gpt-5.4-mini` to the `high` variant so live TUI runs can surface
visible `Thinking:` traces. The signoff helpers still force `gpt-5.4-mini` onto the `low`
variant so the Batch 1 parity lanes stay on the documented low-reasoning path. Set
`HARNESS_LIVE_PROXY_VARIANT` to override the helper default.

Minimal portable baseline:

- a local environment that can launch the harness binary under `portable_pty`
- a reachable configured live proxy/provider
- the bundled renderer path used by `LiveVisualRun`

## Batch 1 parity signoff

When the change is tied to the canonical signoff map from `docs/testing.md`, prefer the composed
Batch 1 parity wrappers first:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_prompt_parity_signoff -- --ignored --exact

HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts \
cargo test -p harness-testkit live_proxy_e2e_tui_parity_signoff -- --ignored --exact
```

These wrappers chain the shipped live lanes instead of inventing a second verification category:

- CLI: `live_proxy_prompt_responses_smoke` → `live_proxy_prompt_chat_tool_flow` → `live_proxy_prompt_native_tool_flow` → `live_proxy_prompt_compat_edit_flow`
- TUI: `live_proxy_preflight` → `live_proxy_e2e_tui_prompt_responses_smoke` → `live_proxy_e2e_tui_tool_flow`

Batch 1 live parity signoff is scoped to the selected
`HARNESS_LIVE_PROXY_PROVIDER` / model / variant tuple. The live helpers record that tuple in the
manifest metadata and summarize observed provider-turn behavior in `run_summary.json` /
`run_summary.txt` so later provider work builds on explicit evidence instead of treating one
provider run as universal proof.

## Main live tool-flow test

Run the prompt-based chat-control lane first when the change is about tool orchestration, todo/question state, or agent workflow helpers:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_prompt_chat_tool_flow -- --ignored --exact
```

This lane exercises a real model against prepared live agents for:

- `todowrite`
- `question`
- `skill`

When the selected model exposes the documented `low` variant, the prepared signoff
agents prefer it automatically so `gpt-5.4-mini` stays on the low-reasoning parity path.

The repo now ships `rust-best-practices` in `.agents/skills`, and the prepared live chat-tool lane
copies that skill into its temporary workspace before the `skill` stage runs. A fresh checkout
therefore does not depend on an externally installed skill. You can still override it by placing a
same-named skill earlier in the configured project-root search order.

The live lane examples below use `configs/harness.example.jsonc` explicitly via
`HARNESS_LIVE_PROXY_CONFIG`; the harness CLI does not auto-discover that file unless you copy it to
`./harness.jsonc`.

Then run the headless native tool-flow lane:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_prompt_native_tool_flow -- --ignored --exact
```

This keeps the headless signoff aligned with the same `fs.write` → `fs.read` →
`edit.hashline_scan` → `edit.hashline_apply` → `fs.read` path that the TUI live lane exercises.

Then run the compat file-edit lane:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_prompt_compat_edit_flow -- --ignored --exact
```

Then run the file-edit / visual lane:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
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

Recommended local root for native screenshot, offline PTY, and live manifest inspection:

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

The native screenshot lane writes sibling runs under:

```text
target/pty-visual-artifacts/native-visual/native_visual_ghostty_smoke/<run-id>/
```

Those runs use the same manifest filenames plus `native_visual_summary.json` / `.txt` so window
capture provenance and cleanup state stay reviewable next to the screenshots.

Example run id:

```text
run-20260307-081508-190134Z
```

Each run directory includes:

- `live_proxy_startup.png`
- `live_proxy_draft_visible.png`
- `live_proxy_file_write_finished.png`
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
3. `live_proxy_prompt_native_tool_flow`
4. `live_proxy_prompt_compat_edit_flow`
5. `live_proxy_prompt_parity_signoff` when you want the full prompt/CLI Batch 1 closeout
6. `live_proxy_e2e_tui_tool_flow`
7. `live_proxy_e2e_tui_parity_signoff` when you want the full TUI Batch 1 closeout
8. `live_proxy_e2e_visual_verifier` only for screenshot/signoff work

The tests are still live-model dependent, so retries are expected and already built into the
TUI tool-flow lane.

When provider behavior differs, record it under the selected provider name in the live summary
evidence and, when it becomes part of signoff, in the provider-turn expectations helper instead of
loosening assertions globally.

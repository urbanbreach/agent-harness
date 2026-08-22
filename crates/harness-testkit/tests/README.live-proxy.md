# Live proxy E2E lane

This lane retains env-gated live proxy signoff entrypoint names after T5 slimming. The current
wrappers verify live prerequisites and documented provider/model selection; provider/tool-flow
behavior is owned by deterministic tests listed in `docs/testing/testing.md` unless a human explicitly runs
and records separate live evidence.

## Preflight

Run this first when validating local setup:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=harness.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=umans-ai-coding-plan \
HARNESS_LIVE_PROXY_MODEL=umans-kimi-k2.7 \
cargo nextest run -p harness-testkit live_proxy_preflight_requires_live_env -- --ignored --exact
```

The preflight verifies:

- live config path resolves
- live env gating is explicit
- the documented default provider/model tuple is visible in the test target

The live proxy smoke lane does **not** require KDE, `konsole`, `spectacle`, a desktop session, or a
local POSIX shell. `live_proxy_preflight_requires_live_env` and the retained prompt/TUI signoff
names verify only the slim env/config prerequisites now retained in T5.

When the workspace `harness.jsonc` is the active live config, the interactive `build` profile
defaults to `umans-ai-coding-plan/umans-kimi-k2.7` so live TUI runs dogfood the Umans coding model.
The signoff helpers use the documented Umans provider/model tuple unless `HARNESS_LIVE_PROXY_PROVIDER`,
`HARNESS_LIVE_PROXY_MODEL`, or `HARNESS_LIVE_PROXY_VARIANT` override it.

Minimal portable baseline:

- a reachable configured live proxy/provider
- the workspace `harness.jsonc` provider/model tuple, unless overridden by env

## Live signoff

For an explicit live-provider check, use the slim signoff entrypoints:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=harness.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=umans-ai-coding-plan \
HARNESS_LIVE_PROXY_MODEL=umans-kimi-k2.7 \
cargo nextest run -p harness-testkit live_proxy_prompt_signoff -- --ignored --exact

HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=harness.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=umans-ai-coding-plan \
HARNESS_LIVE_PROXY_MODEL=umans-kimi-k2.7 \
HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts \
cargo nextest run -p harness-testkit live_proxy_e2e_tui_signoff -- --ignored --exact
```

These wrappers are the shipped slim live signoff entrypoints:

- CLI: `live_proxy_preflight_requires_live_env` → `live_proxy_prompt_signoff`
- TUI: `live_proxy_preflight_requires_live_env` → `live_proxy_e2e_tui_signoff`

Live signoff is scoped to the selected `HARNESS_LIVE_PROXY_PROVIDER` / model /
variant tuple. After T5 slimming, these wrappers only assert the prerequisite tuple and config path;
they do not write live manifests or summarize provider-turn behavior.

## Retired full live tool-flow tests

The previous prompt chat/native-tool/TUI tool-flow matrix was retired during T5 slimming. Its
behavioral assertions are now owned by deterministic provider cassette, harness-tools native parity,
and harness-tui render/view-model tests listed in `docs/testing/testing.md`. T5 retains only the explicit
env-gated live signoff names above.

## Artifact layout

The slim live proxy wrappers (`live_proxy_preflight_requires_live_env`,
`live_proxy_prompt_signoff`, `live_proxy_e2e_tui_signoff`) do **not** produce live
artifact trees (no `run-*` directories, no `manifest.json` / `run_summary.*` from T5 preflight
wrappers). That is intentional: this lane is slim signoff names only and does **not** own the
native tool behavioral matrix.

A **budgeted live smoke pack** with redacted evidence is available separately via
`scripts/harness-qa-live-smoke.sh` (residual PRD WS-L1) and the `harness-qa` skill live channel
(WS-L2). Evidence lands under gitignored
`artifacts/qa-evidence/<YYYYMMDD>-live-<slug>/` (README, commands.log, isolation-receipt,
budget-receipt, events-excerpt, secret-scan, lane-or-run-summary). Fail-closed without live env:
`bash scripts/harness-qa-live-smoke.sh --self-test-fail-closed` exits non-zero. Live smoke proves
transport/auth/fixed short prompts only; optional tool smoke (`HARNESS_LIVE_SMOKE_TOOL=1`) is not
matrix ownership.

The layout below is retained for historic full live visual runs and lane-stage artifacts under
`target/test-lanes/` / `target/pty-visual-artifacts/`.

Artifacts are written under:

```text
<artifact-root>/live-proxy/<test-name>/<run-id>/
```

Recommended local root for native screenshot, offline PTY, and live manifest inspection:

```text
target/pty-visual-artifacts/
```

The native screenshot lane writes sibling runs under:

```text
target/pty-visual-artifacts/native-visual/native_visual_ghostty_smoke/<run-id>/
```

Those runs use the same manifest filenames plus `native_visual_summary.json` / `.txt` so capture
provenance and cleanup state stay reviewable next to the screenshots.

Example run id:

```text
run-20260307-081508-190134Z
```

Historic full visual runs included:

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

1. `live_proxy_preflight_requires_live_env`
2. `live_proxy_prompt_signoff` when you want the CLI live signoff name
3. `live_proxy_e2e_tui_signoff` when you want the TUI live signoff name

The tests are live-model dependent and intentionally opt-in. Deterministic behavior assertions live
outside this T5 lane.

When provider behavior differs, record it under the selected provider name in the live summary
evidence and, when it becomes part of signoff, in provider cassette expectations instead of
loosening assertions globally.

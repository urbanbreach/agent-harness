# Live proxy checks

The `live_proxy_e2e` target checks the environment, config path, and selected
provider/model tuple. Its retained prompt and TUI signoff wrappers do not run a
full provider/tool journey or produce live artifact trees.

Use [`scripts/harness-qa-live-smoke.sh`](../../../scripts/harness-qa-live-smoke.sh)
for a budgeted live authentication and transport check. Native tool behavior is
covered by the deterministic tests in the [testing guide](../../../docs/testing/testing.md).

## Preflight

Set the config path and IDs to values present in your configuration. This example
uses the Umans tuple used by the test helpers:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=harness.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=umans-ai-coding-plan \
HARNESS_LIVE_PROXY_MODEL=umans-kimi-k2.7 \
cargo nextest run -p harness-testkit --test live_proxy_e2e \
  --ignore-default-filter --run-ignored only \
  -E 'test(=live_proxy_preflight_requires_live_env)'
```

Preflight checks explicit opt-in, the config path, and provider/model selection.
It does not require KDE, Konsole, Spectacle, or a desktop session. The helpers
accept `HARNESS_LIVE_PROXY_PROVIDER`, `HARNESS_LIVE_PROXY_MODEL`, and optional
`HARNESS_LIVE_PROXY_VARIANT` overrides. Do not infer the current interactive
model from the helper defaults; inspect your effective config.

## Signoff wrappers

With the same environment, run preflight before either wrapper:

```bash
cargo nextest run -p harness-testkit --test live_proxy_e2e --ignore-default-filter --run-ignored only -E 'test(=live_proxy_prompt_signoff)'
cargo nextest run -p harness-testkit --test live_proxy_e2e --ignore-default-filter --run-ignored only -E 'test(=live_proxy_e2e_tui_signoff)'
```

These wrappers check prerequisites for the selected tuple. A passing result does
not establish live provider-turn behavior. The old full prompt, native-tool, and
TUI matrix was removed during the T5 test reduction.

## Live smoke evidence

The separate smoke script writes redacted evidence under the ignored directory
`artifacts/qa-evidence/<YYYYMMDD>-live-<slug>/`. It records commands, isolation and
budget receipts, event excerpts, a secret scan, and a run summary.

```bash
# This check must fail when live prerequisites are absent.
bash scripts/harness-qa-live-smoke.sh --self-test-fail-closed

# Supply the live environment before running this command.
bash scripts/harness-qa-live-smoke.sh --slug provider-check
```

The smoke checks authentication, transport, and fixed short prompts. Optional
`HARNESS_LIVE_SMOKE_TOOL=1` adds one tool check; it does not replace the native
tool suite. The `harness-qa` skill also provides this live workflow.

## Historical visual artifacts

Older full visual runs used:

```text
<artifact-root>/live-proxy/<test-name>/<run-id>/
```

Those runs could include startup, draft, edit, scan, and completion screenshots,
`manifest.json`, `manifest.jsonl`, `run_summary.json`, and `run_summary.txt`.
The current prerequisite wrappers do not create these files.

Local PTY and native captures use `target/pty-visual-artifacts/`. Native runs use
`native-visual/native_visual_ghostty_smoke/<run-id>/` and can add
`native_visual_summary.json` and `.txt` alongside the manifest.

Historical retention keeps five screenshot runs per test by default.
`HARNESS_LIVE_VISUAL_KEEP_RUNS` overrides the count. Pruning affects only
manifest-backed `run-*` directories and leaves sidecars alone.
`HARNESS_LIVE_VISUAL_VIEWPORT` selects `desktop`, `laptop`, or `compact`.

Keep any provider-specific failure attached to the selected provider and the
recorded run. Do not loosen deterministic expectations to accept a different
live response.

# Privacy and local data

Harness is local-first. It writes sessions, artifacts, config, prompts, and skills locally. Configured provider/MCP calls, explicit network tools, and automatic model-catalog downloads can generate outgoing traffic.

## Data egress

Configured provider requests and enabled MCP server calls can send data out. `webfetch`, `websearch`, and `codesearch` are explicit tool calls under permission policy.

Ordinary live catalog initialization can download model metadata from `https://models.dev/api.json`. A valid cache is reused for five minutes; a valid stale cache is served immediately while refreshing in the background. Without a usable cache, initialization attempts a download and falls back to bundled metadata on failure. `HARNESS_MODELS_URL` and `HARNESS_MODELS_PATH` override the source and cache location; `HARNESS_DISABLE_MODELS_FETCH=1` selects only the embedded catalog. Mock TUI model-picker initialization always uses the embedded catalog without invoking this environment-backed loader.

Replay, session inspection, doctor, and support export remain local/offline; live provider checks require a separate operator action.

## Storage paths

Runtime config lives in `harness.json` / `harness.jsonc` under XDG config or project-local paths. TUI config lives in `tui.json` / `tui.jsonc`. Project prompt assets and skills live under `.agent-harness/agents` and `.agent-harness/skills`. Session logs and artifacts live under the configured session directory and per-run artifact directories.

## Redaction

Redaction is implemented in `crates/harness-core/src/redact.rs`. Support export includes a support export redaction manifest and scans for API keys, bearer tokens, cookies, PEM blocks, raw provider credentials, and hidden prompt/config instruction values. Share the support bundle instead of raw `events.jsonl` when possible.

## No telemetry

There is no telemetry, cloud analytics, billing, web share, or hosted collaboration surface in V1 unless explicitly added later by a new roadmap item and implementation. Doctor does not make provider network calls.


## Operator checklist

1. Review provider/MCP config before live calls.
2. Prefer mocked prompt tests for deterministic evidence.
3. Export redacted support bundles for debugging.
4. Treat approved `bash` and `edit` actions as local mutation authority.

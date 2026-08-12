# Privacy and local data

Harness is local-first. It writes local sessions, artifacts, config, prompts, and skills, and it sends data out only through explicitly configured provider or MCP calls.

## Data egress

The only routine data egress paths are configured provider requests and enabled MCP server calls. `webfetch`, `websearch`, and `codesearch` are also explicit tool calls under permission policy. Replay, session inspection, doctor, and support export are local/offline unless an operator runs a live provider lane.

## Storage paths

Runtime config lives in `harness.json` / `harness.jsonc` under XDG config or project-local paths. TUI config lives in `tui.json` / `tui.jsonc`. Project prompt assets and skills live under `.agent-harness/agents` and `.agent-harness/skills`. Session logs and artifacts live under the configured session directory and per-run artifact directories.

## Redaction

Redaction is implemented in `crates/harness-core/src/redact.rs`. Support export includes a support export redaction manifest and scans for API keys, bearer tokens, cookies, PEM blocks, raw provider credentials, and hidden prompt/config instruction values. Share the support bundle instead of raw `events.jsonl` when possible.

## No telemetry

There is no telemetry, cloud analytics, billing, web share, or hosted collaboration surface in V1 unless explicitly added later by a new roadmap item and implementation. Doctor does not make provider network calls.

### Local presentation-fidelity QA evidence

The opt-in TUI fidelity runner may write local presentation receipts only when its runner-owned
evidence path is supplied. This is local-only QA evidence, not product telemetry: it has no network
path and is not enabled by runtime or TUI configuration. The records use content-free interaction
and cause IDs plus timestamps, byte lengths, SHA-256 digests, decoder state, semantic frames, and
cleanup/provenance metadata. They must not include raw user input, provider content, credentials,
cookies, or hidden reasoning. Operators should keep the generated artifact directory local, review
it before sharing, and use the normal redacted support export for support cases.

## Operator checklist

1. Review provider/MCP config before live calls.
2. Prefer mocked prompt tests for deterministic evidence.
3. Export redacted support bundles for debugging.
4. Treat approved `bash` and `edit` actions as local mutation authority.

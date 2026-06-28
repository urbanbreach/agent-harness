# Troubleshooting first-run Harness

Use this checklist when the V1 install-to-first-edit path fails. `doctor` is a
local readiness check only; use `prompt`, live stress lanes, or support bundles
for execution proof.

| Symptom | Check | Evidence to keep |
|---|---|---|
| Missing credentials | Run `harness doctor` and set the reported `apiKey` or `apiKeyEnv` names. | Doctor text/JSON. |
| Invalid credentials or rate limits | Run a live `prompt` or live stress lane; `doctor` does not call the provider. | Prompt stderr, `events.jsonl`, support export. |
| Base URL or local proxy mismatch | Compare provider `baseURL` with the local proxy endpoint. | Config path, doctor JSON, prompt transport error. |
| Missing MCP/LSP/tool prerequisites | Check doctor MCP rows and persisted tool failures. Unsupported LSP probes should be recoverable tool messages. | Tool event rows and artifact index. |
| Missing MCP server | Confirm the `mcp` entry is enabled, the command exists, and doctor reports the server as ready. | Doctor JSON and MCP stderr artifact if any. |
| Unsupported tool call | Inspect the session read-only and review the failed tool output. | `harness sessions inspect <run> --json`. |
| Malformed provider stream | Keep the provider error event and sanitized context; do not share raw provider payloads. | Redacted support bundle. |
| Session resume failure | Run `harness sessions inspect <run>` and check `resume_disabled_reason`. | Inspect JSON and session path. |
| Replay failure | Confirm `events.jsonl` exists and has contiguous sequence numbers. Replay never executes tools, providers, hooks, or MCP. | Replay stderr/JSON. |
| permission denial or timeout | Look for `permission_resolved`, `edit_rejected`, and failed tool output; the denied edit should not change the file. | Event log plus workspace diff. |
| terminal rendering issues | Retry with `--mock`, use `/help` or `Ctrl+p`, and include terminal size/emulator details with a redacted bundle. | TUI screenshot/transcript and support bundle. |

For support, export the session rather than sharing raw event logs:

```bash
harness sessions export --session-dir <session-dir> --output support-bundle.json <run-id>
```

The export redacts known token shapes and fails closed if exact config
credentials or hidden prompt/config instruction values survive redaction.

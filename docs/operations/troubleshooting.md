# Troubleshooting

Start with `harness config validate` and `harness doctor`. Both run locally.
A passing doctor report confirms local readiness, not working authentication or
a reachable provider. Use one live `harness run` to check those.

| Problem | What to check |
| --- | --- |
| A setting seems ignored | Run `harness config sources`, then `harness config explain <path>`. A later layer may override the value. |
| Credentials are missing | Follow the provider row in `harness doctor`. For the starter, use `harness auth login codex` or set `OPENAI_API_KEY`. |
| The provider rejects credentials | Check the selected account and endpoint. Keep the sanitized error and a support export. |
| The provider rate-limits requests | Wait or reduce request volume. Check the error category before changing credentials. |
| A local proxy fails | Compare the configured `baseURL` with the proxy's listening address. |
| An MCP server is unavailable | Confirm the entry is enabled and its executable exists. Doctor checks configuration; it does not connect to MCP. |
| An LSP operation is unavailable | Read the tool's structured error and check the server executable. Use compiler diagnostics when the server cannot report them. |
| A tool is denied | Check the tool's permission kind and matching rule order. The last matching rule wins. Inspect the workspace diff if the request involved an edit. |
| A provider stream fails | Keep the error category and support bundle. Do not share raw provider payloads. |
| A session cannot resume | Run `harness sessions inspect <run-id-or-path> --json` and read `resume_disabled_reason`. |
| Replay fails | Check that `events.jsonl` exists and its sequence is valid. Inspect the reported error before attempting recovery. |
| The TUI renders incorrectly | Retry with `harness tui --mock`. Record the terminal emulator, dimensions, and a screenshot. `Ctrl+p` opens the command palette. |

## Share a support bundle

```bash
harness sessions export --session-dir <session-dir> --output support-bundle.json <run-id>
```

The exporter redacts known secret formats, then scans for credentials and hidden
prompt or config values. A failed scan prevents it from writing the bundle.
Share the bundle rather than raw events. See [privacy and local data](../permissions/privacy-and-local-data.md).

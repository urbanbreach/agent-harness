# Configuration

Agent Harness loads `harness.jsonc` from the current working directory by default, or
from `$XDG_CONFIG_HOME/harness/config.jsonc` when no local file is present. You can
validate the resolved file with:

```bash
cargo run -p harness -- config validate
```

## Integrations

The public `integrations` surface is intentionally narrow today.

- Supported now: `integrations.remote_search`
- Backed by: the built-in `web_search` and `code_search` tools
- Not public yet: generic `integrations.mcp.servers` registration for arbitrary MCP servers

If you need to configure external search today, use `remote_search`:

```json5
{
  integrations: {
    remote_search: {
      endpoint: "https://mcp.exa.ai/mcp",
      auth_token: "${EXA_API_KEY}",
      require_auth: true,
      timeout_secs: 30,
      max_retries: 1,
      retry_backoff_ms: 250
    }
  }
}
```

### `integrations.remote_search`

`remote_search` configures the current runtime bridge used by native search tools. The
runtime expects an Exa-compatible MCP endpoint and does not expose a general-purpose
MCP server registry through config yet.

| Field | Default | Meaning |
| --- | --- | --- |
| `endpoint` | `https://mcp.exa.ai/mcp` | MCP endpoint used for built-in remote search requests |
| `auth_token` | `null` | Optional bearer token sent to the endpoint |
| `require_auth` | `false` | Fail fast when no auth token is configured |
| `timeout_secs` | `30` | Per-request timeout |
| `max_retries` | `1` | Retry count for retryable failures |
| `retry_backoff_ms` | `250` | Delay between retries |

Environment overrides are also supported:

- `HARNESS_REMOTE_SEARCH_ENDPOINT` or `HARNESS_EXA_MCP_ENDPOINT`
- `HARNESS_REMOTE_SEARCH_AUTH_TOKEN`, `HARNESS_EXA_MCP_AUTH_TOKEN`, or `EXA_API_KEY`
- `HARNESS_REMOTE_SEARCH_REQUIRE_AUTH`
- `HARNESS_REMOTE_SEARCH_TIMEOUT_SECS`
- `HARNESS_REMOTE_SEARCH_MAX_RETRIES`
- `HARNESS_REMOTE_SEARCH_RETRY_BACKOFF_MS`

Until generic MCP configuration is shipped, docs, examples, and schema should all be
read as describing only this native remote-search integration surface.

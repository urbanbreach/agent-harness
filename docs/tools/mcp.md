# MCP tools

Configure servers under `integrations.mcp.servers`:

```jsonc
{
  "integrations": {
    "mcp": {
      "servers": {
        "local": {
          "transport": "stdio",
          "command": ["my-mcp-server"],
          "timeout_secs": 30,
          "enabled": true
        },
        "docs": {
          "transport": "http",
          "endpoint": "https://example.com/mcp",
          "timeout_secs": 30,
          "enabled": false
        }
      }
    }
  }
}
```

Stdio servers accept `env` and `cwd`. Relative working directories resolve against
the session workspace. HTTP servers accept a `headers` map, including authorization
headers. Redirects and automatic replay of failed HTTP calls are disabled.

Registry construction, replay, and inspection do not connect to servers. The first
approved call opens a connection. Later calls in the same run reuse it. Each server
handles one call at a time; waiting for that slot counts against the call timeout.
Stopping a run closes its connections and terminates stdio process groups. Stdio
currently requires Unix process-tree control.

Each enabled server exposes six operations:

| Tool suffix | Arguments |
| --- | --- |
| `tools.list` | `{}` |
| `tool.call` | `tool`, optional object `arguments` |
| `resources.list` | `{}` |
| `resource.read` | `uri` |
| `prompts.list` | `{}` |
| `prompt.get` | `name`, optional string-valued object `arguments` |

For example, `mcp.local.tool.call` invokes a named tool on `local`. A successful
`mcp.local.tools.list` also installs the discovered argument schemas. Discovered
tools become available to the provider in the same agent turn. ASCII identifiers
such as `echo` use `mcp.local.echo`; names requiring normalization include a digest
suffix. The provider receives separate protocol-safe aliases. Durable history and
permission checks retain the harness tool IDs.

Provider requests contain at most 128 tool definitions. Explicit profile tools
take priority, including discovered tools named directly in the profile. Remaining
slots use discovered tools in catalog order. The full catalog stays available
through `tools.list` and `tool.call`, so a large server does not make a turn fail
or lose access to its other tools. More than 128 explicit tools produces a setup
error for that turn.

A server tool-list notification invalidates cached schemas. It does not start a
new discovery request; the next approved `tools.list` call refreshes the catalog.

Primary profiles receive the configured server operations during CLI setup.
Native `task` and `sonic` children inherit permitted MCP operations; the bundled
research and review specialists exclude MCP. Custom definitions control their own
MCP inheritance. A profile
with a server's `tool.call` operation can use its discovered tools. Enabling a tool
does not grant permission to run it.

Generic and direct invocations check both the operation and the named tool. A deny
on `mcp.local.echo` also blocks `mcp.local.tool.call` with `tool: "echo"`. MCP servers
run with the authority of their process or remote service; these checks are not an
OS sandbox.

Responses are limited to 1 MiB. Lists stop at 1,024 entries or 32 pages and reject
repeated cursors. Timeouts must be between 1 and 300 seconds. Cancellation sends an
MCP cancellation notification. Native shutdown gives the process group a short
termination period before killing remaining processes and reaping the server.

Results retain text and structured content. Resource lists include their URIs.
Images in tool results, resources, and prompts reach the provider as attachments.
Their bytes stay outside the journal and structured output, in private attachment
blobs. The existing 1 MiB MCP response limit includes base64 data. Remote resource
links do not trigger an implicit download. Audio and other binary resources remain
omitted.
Configured literal credentials pass through coordinator redaction, and application
logs exclude SDK protocol traces that can contain raw remote payloads.

The implementation uses the [official Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)
for MCP sessions and protocol messages. The harness bounds the byte streams and
owns permission checks, tool registration, cancellation, and run cleanup.

Local HTTP/SSE and native-process checks cover this implementation. An opt-in live
check also passed catalog discovery and native web/code searches against Exa's
public MCP server. This does not verify arbitrary third-party server behavior.

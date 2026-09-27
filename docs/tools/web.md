# Web tools

## Fetching

`webfetch` fetches text, images, PDFs, and other binary responses over HTTP or HTTPS. Pass `url` and an
optional `format` of `markdown`, `text`, or `html`. The default is `markdown`.
`timeout` is in seconds and clamps to 1 through 120, with a default of 30.

The coordinator checks the submitted URL and its normalized form before the
request. Embedded credentials and other URL schemes are rejected. Redirects are
limited to ten. Each destination must pass the current `webfetch` policy before
any request reaches it. A destination that needs separate approval must be
submitted as a new call. Redirects cannot override a deny rule.

The response limit is 5 MiB. Declared lengths and streamed bytes are checked.
Gzip, Brotli, and deflate responses are decoded by `reqwest`; the streamed limit
applies to decompressed bytes. Text uses the declared character set or UTF-8,
with byte-order-mark detection. Invalid text, unknown character sets, and decoded
text above 5 MiB are rejected.

HTTP errors do not return the server's error body. HTML conversion
uses `htmd`, with script, style, head, noscript, and iframe content removed from
text and Markdown output. Raw HTML output preserves the fetched HTML as text.
Conversion runs outside the async executor and is joined before the tool ends.

Results include the final URL, content type, requested format, and fetched byte
count. The coordinator redacts display text and metadata, then applies its normal
inline and artifact limits. Cancellation drops the active request and waits for
any conversion already in progress.

PNG, JPEG, GIF, and WebP responses become attachments. The coordinator checks
their metadata, headers, digests, and known credentials before retaining private
blobs. Journal records do not contain payload bytes. PDFs and other binary responses are retained as private, digest-named artifacts. Results explicitly state that contents were not extracted; provider requests receive the artifact reference rather than the binary payload. Detected credentials reject the entire artifact before any write. This byte scan does not inspect compressed or encrypted document contents.

The deterministic HTTP fixture covers conversion, images, binary and PDF retention, credential rejection, character-set decoding,
gzip decoding and expansion limits, redirects, permission denial, error handling,
and cancellation.

## Search

`websearch` and `codesearch` send approved queries to Exa using the same lazy MCP
transport as configured servers. They share one connection per run. Registration,
replay, and inspection remain offline. Queries must contain nonblank text and fit
within 4,096 bytes.

| Tool | Options |
| --- | --- |
| `websearch` | `numResults`: 1–20, default 8. `type`: `auto`, `fast`, or `instant`. `livecrawl`: `fallback` or `preferred`. `contextMaxCharacters`: 1–50,000, default 10,000. |
| `codesearch` | `tokensNum`: default 5,000, clamped to 1,000–50,000. This is an approximate output budget of four characters per token. |

Snake-case aliases are accepted for the numeric options. Search output includes
source URLs and a truncation flag. Redaction happens before shortening text so a
cut cannot expose part of a configured credential. The normal coordinator output
and artifact limits also apply.

Both tools use `web_search_advanced_exa`. For the hosted `mcp.exa.ai` endpoint, the
harness adds that tool to the URL when no explicit `tools` parameter exists.
Custom endpoints and explicit tool lists must provide it. `preferred` live crawling
maps to `maxAgeHours: 0`; `fallback` uses the service default. Exa's current search
types replace the obsolete `deep` option. The current contract is documented in
[Exa's MCP guide](https://exa.ai/docs/get-started/exa-mcp).

Configure `integrations.remote_search` with `endpoint`, optional `auth_token`,
`require_auth`, `timeout_secs`, `max_retries`, and `retry_backoff_ms`. Tokens use the
`x-api-key` header. A required but missing token fails before making a request.
Timeouts clamp to 1–120 seconds per attempt. Only HTTP 429 and 5xx responses are
retried, up to five retries. Retry delays use `Retry-After` when available, otherwise
the configured backoff, capped at five seconds. Cancellation interrupts that wait.
Authentication and MCP application errors are not retried.

CLI environment values override file configuration:

| Setting | Environment names, in precedence order |
| --- | --- |
| Endpoint | `HARNESS_REMOTE_SEARCH_ENDPOINT`, `HARNESS_EXA_MCP_ENDPOINT` |
| Token | `HARNESS_REMOTE_SEARCH_AUTH_TOKEN`, `HARNESS_EXA_MCP_AUTH_TOKEN`, `EXA_API_KEY` |
| Require token | `HARNESS_REMOTE_SEARCH_REQUIRE_AUTH` |
| Attempt timeout | `HARNESS_REMOTE_SEARCH_TIMEOUT_SECS` |
| Retries | `HARNESS_REMOTE_SEARCH_MAX_RETRIES` |
| Backoff | `HARNESS_REMOTE_SEARCH_RETRY_BACKOFF_MS` |

The opt-in external check passed MCP catalog discovery and both native searches
against the public Exa service. Run it with:

```bash
HARNESS_MCP_LIVE_SIGNOFF=1 cargo nextest run --profile ci -p harness-tools \
  --test live_proxy_e2e --ignore-default-filter
```

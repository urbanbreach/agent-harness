# Log in to a provider

Run `harness` without a config file, then use `/login` or choose `Login` from
the command palette. Both open the provider picker. It offers OpenAI ChatGPT
Plus/Pro or an API key, GitHub Copilot device login, Anthropic API key, Claude
Pro/Max subscription, Google, OpenRouter, and more. You can also
use `harness auth login <provider>` or export a provider API key before startup.

After a successful CLI or TUI sign-in stores a credential, Harness writes a
minimal `<home>/harness.jsonc` if neither user runtime config file exists and
`HARNESS_CONFIG` and `HARNESS_CONFIG_CONTENT` are unset. `<home>` is a nonempty
`HARNESS_HOME` used as-is, otherwise `~/.harness`. The write uses create-new
semantics and POSIX mode `0600`, never overwrites a file, and prints
`wrote starter config: <path>` on stdout.

The file includes the schema URL and comments explaining the one-time write and
pointing to `harness models`. Its `model` is the default for the provider just
signed in, if known; otherwise the key is omitted. It has no `provider` entries,
so automatic discovery stays enabled. Exporting an API key alone does not write
this file. Keep project policy in `<project>/harness.jsonc`; it overrides personal
defaults. See [config precedence](../configuration/config.md#discovery-and-precedence).

Credentials are stored as private JSON files at
`<home>/credentials/<credential-provider-id>.json`, using atomic writes. For
Codex, this is `credentials/codex.json`, not `credentials/openai-codex.json`.
The Claude account pool is in `credentials/anthropic-subscription.json`.
`harness auth logout <provider>` deletes that stored credential. It does not
unset environment keys or clear a host Claude Code login.

API-key login defaults to a hidden terminal prompt. Without a terminal, pass
`--api-key-stdin` and supply the key on standard input. Codex defaults to browser
login; Copilot defaults to device login. Both
`harness auth login openai --method browser` and
`harness auth login openai --method device` store a Codex credential.
`harness auth login openai --method api-key` stores an OpenAI API key.
Copilot accepts `--enterprise-url` for an Enterprise domain.

```bash
harness auth login codex --method browser
harness auth login codex --method device
harness auth login github-copilot --method device
harness auth list --json
harness auth logout codex
```

`openai-codex` is also accepted as the Codex credential name. A custom provider's
`authProvider` selects its credential identity. For example, `authProvider:
"codex"` uses the same stored account as the built-in subscription provider.

For HTTP providers, stored credentials take precedence over environment and
inline keys. An expired stored OAuth credential must refresh successfully; it
does not silently fall through to another account. Credentials join the run's redaction registry
before use. Refresh retains old secrets in that registry so delayed output cannot
expose them.

## Inspect connections and models

```bash
harness auth list
harness auth list --json
harness providers protocols
harness models
harness models list --json
harness doctor --json
```

`auth list` reports stored credentials, not environment-key availability or
live authentication. Its JSON includes credential kind and expiry, never
tokens. `providers protocols` reports implemented HTTP protocols, not accounts.
`harness models` lists resolved model selections and token-limit provenance.
`harness models generated` prints the bundled catalog. `harness models probe`
reads a catalog and prints filtered provider configuration.
`harness models generate` writes it, by default to
`configs/provider-catalog.generated.json`. Neither probes account access.
Doctor checks local readiness without network requests or file creation.

## Claude subscription accounts

Connect `anthropic-subscription` with browser login or copy-code login. In the
CLI, `--method device` selects the copy-code flow, not device polling. Every
turn runs through a local Claude Code binary. Harness does not install it.
Set `CLAUDE_CODE_EXECUTABLE` to its path, put `claude` on `PATH`, or use its
standard `~/.local/bin/claude` or `~/.claude/local/claude` install location.

```bash
harness auth login anthropic-subscription --method browser
harness auth login anthropic-subscription --method copy-code
harness auth claude-account list
harness auth claude-account add
harness auth claude-account pin <id>
harness auth claude-account unpin
harness auth claude-account rename <id> <display-name>
harness auth claude-account clear-name <id>
harness auth claude-account remove <id>
```

The first login uses account ID `default`; later logins can add named accounts.
Use `claude-account list` to see account sources, status, pins, and the affinity
selection. Omitting `list` also lists accounts. The TUI has `/claude-account`.

Environment slots `CLAUDE_CODE_OAUTH_TOKEN` and `CLAUDE_CODE_OAUTH_TOKEN_2`
through `CLAUDE_CODE_OAUTH_TOKEN_16` also enable discovery. A host Claude Code
login alone does not: explicitly configure an `anthropic_subscription` provider
with `enabled: true` to opt in to the ambient account.

In a resumed main session Claude Code owns the context, so Harness skips its own
automatic compaction and the input-budget stop. Claude Code compacts the session
against the model's catalog window, passed as its `autoCompactWindow` setting.
Claude Code only accepts 100K to 1M, so a smaller window is raised to 100K. Each
compaction leaves a transcript row such as `Claude Code compacted this
conversation (76K → 8.9K tokens)`.

## Choose a transport

Provider definitions use `type: "openai_compatible"`, `type: "anthropic_messages"`,
or `type: "anthropic_subscription"`. The subscription type runs Claude Code
instead of making HTTP requests from Harness. For OpenAI-compatible servers,
`apiMode` selects the wire format:

| Value | Requests |
| --- | --- |
| `responses` | Responses API only |
| `chat_completions` | Chat Completions only |
| `auto` | Responses first; one Chat Completions attempt after HTTP 404/405, or HTTP 400 from loopback |

Automatic fallback happens before streaming starts. Authentication errors,
transport failures and malformed streams do not trigger a protocol switch.
Coordinator retry and configured model fallback remain separate decisions.
HTTP redirects are rejected. Response failures expose status and category without
including raw server bodies or request credentials. Endpoint construction and
fallback preserve base-URL query parameters. Query values join the redaction
registry in both encoded and decoded forms.

Codex authentication always uses Responses and cannot fall back to Chat
Completions. Known OpenAI service hosts map to
`https://chatgpt.com/backend-api/codex/responses`; an explicit custom host stays
configured. Requests carry account and session headers, put system text in
`instructions`, and set `store: false`. The server controls the output limit;
the request budget records this instead of claiming a client-enforced limit.
GPT-6 Astra defaults to low reasoning effort, automatic summaries and low text
verbosity. Explicit settings take precedence. Encrypted reasoning is requested
but never written into the journal.

Codex catalogs include only the verified subscription model IDs. Retired
GPT-5.4 models are rejected before a provider call. The built-in default is the
bundled GPT-6 Astra entry. Actual availability still depends on the account and
workspace. The OpenAI API's separate model availability is unchanged by this
subscription filter. See the [current Codex model documentation](https://learn.chatgpt.com/docs/models).

Copilot requests carry a bearer credential, request initiator and image-use
headers. The first request for a direct user turn uses the user initiator; tool
follow-ups, delegated tasks and compaction use the agent initiator. A stored
Enterprise domain changes the public Copilot host to `copilot-api.DOMAIN`.
Copilot sends its bearer credential directly to the service. In automatic
mode, Claude models use the native Anthropic Messages endpoint with bearer
authentication. An explicit base URL can select another service endpoint.

## Remote compaction

The `remote` compaction method asks the provider to compact the context. Which
routes a model gets depends on its wire API and host:

| Route | Offered when |
| --- | --- |
| Streaming Responses | The model uses the Responses API, `runtime.compaction.remote_streaming_v2_enabled` is on and the model's `v2_streaming_enabled` is not `false`. Tried first, up to three attempts with 1 s and 2 s between them. |
| Responses compact endpoint | The provider id is `openai`, or `remote_models` sets `enabled: true` for the model. Codex needs an explicit `endpoint`. |
| Anthropic | The model is Claude 4.6 or 4.7 on `api.anthropic.com` or Vertex. Other Claude models, and the Azure AI and AWS routes, need `enabled: true`. |
| Chat Completions | None. |

`runtime.compaction.remote_enabled: false` or `enabled: false` in the model's
`remote_models` entry offers no route at all, and a Codex model must be one of the
allowed subscription models. `api` in a `remote_models` entry picks the wire API
(`responses`, `anthropic` or `chat`), and `model` replaces the model id sent. Without an
`endpoint` or `v2_endpoint`, Harness derives both Responses URLs from the
provider's base URL: `https://api.openai.com/v1/responses/compact` and
`https://api.openai.com/v1/responses` by default, and the
`https://chatgpt.com/backend-api/codex/...` equivalents for Codex. Each request has
a 300-second limit. When every route fails and `remote_endpoint` is set, the pass
falls back to that endpoint; under the `remote` method it never falls back to the
ordinary summarizer.

Native payloads that a route returns are stored as private artifacts; see
[sessions and replay](../architecture/sessions-and-replay.md#compaction-records).

### Credentials

Before each summarizer candidate runs, Harness asks its provider for a credential.
The custom endpoint uses it; ordinary summary requests and handoffs authenticate
through the provider as usual. An HTTP provider returns its API key and the headers
configured for the model. A keyless provider returns an empty credential, so the
request goes out unauthenticated. The Claude subscription provider is keyless on
purpose, because its OAuth tokens never go to a custom endpoint. If the key source
fails, the lookup stops the walk over summarizers with
`Remote compaction credentials unavailable`, classed as an authentication failure.
The same lookup decides whether `context_promotion` may switch to a target model.

### Custom summary endpoint

`remote_endpoint` must be an `http` or `https` URL without a username or password.
Redirects are not followed. The path picks the format:

- A path ending in `/chat/completions` (a trailing slash is fine, a query is
  ignored) gets a Chat Completions request: `messages` with the system prompt and
  the request, `stream: false`, `model` set to the summarizer's model id and
  `max_tokens`. It carries `Authorization: Bearer <key>` when the credential has a
  key, plus the model's configured headers. The reply must hold
  `choices[0].message.content` as a string, or as an array of parts whose `text`
  values are joined.
- Any other path gets `{"systemPrompt", "prompt", "maxTokens"}` with only
  `Content-Type: application/json`; no key and no model headers are sent. The reply
  must be an object with a string `summary` and an optional string `shortSummary`.
  Other fields are kept as received.

Failures use these messages, which also reach the compaction warning:

| Message | Cause |
| --- | --- |
| `invalid summary endpoint` | The URL does not parse, is not http or https, or embeds credentials |
| `Remote compaction credentials unavailable` | The provider could not produce its key |
| `Remote compaction credential header is invalid` | The key or a configured header cannot be sent as an HTTP header |
| `Remote compaction client failed` | The HTTP client could not be built |
| `Remote compaction request invalid` | The request body could not be serialized |
| `Remote compaction connection failed` | The request did not reach the endpoint |
| `Remote compaction failed (<status> <reason>)` | A non-success status, for example `Remote compaction failed (401 Unauthorized)`; 401 and 403 count as authentication failures, 429 as rate limiting |
| `Remote compaction response parse failed` | The body is not JSON |
| `Remote compaction response missing choices[0].message.content` | Chat format reply without text |
| `Remote compaction response missing summary` | Other format reply without a string `summary` |
| `Remote compaction cancelled` | The pass was cancelled |

Failure bodies are redacted, capped at 8 KiB and kept out of the message; the
message names the status only.

## Startup and model limits

Catalog selection is offline. When the loaded config has no provider entries,
it discovers stored credentials and API key environment variables against the
embedded catalog. A config that only sets permissions or other defaults keeps
this behavior. Defining any `provider` entry makes the catalog curated: only
configured providers plus signed-in Codex, GitHub Copilot, and Claude
subscription are included. A provider-less config may still pin a
`provider/model`, resolved through runtime discovery rather than config validation.
A connected pin is preserved; when its provider is disconnected, the TUI uses
another connected provider's default or shows the disconnected `/login` state.
A noninteractive run with no connection reports the pinned provider and points
to `harness auth login`. Project instructions remain authoritative.
Codex also reads missing, supported model entries from `HARNESS_MODELS_PATH` or
`<home>/models-cache.json`. Cached entries never replace
configured models. Invalid caches are ignored. Runtime
selection does not fetch or write a catalog; startup, sign-in and
`harness models refresh` refresh it, as described in
[provider support](../configuration/provider-support.md). `harness models probe`
and `harness models generate` fetch explicitly unless given `--input` or
`--stdin`. `HARNESS_DISABLE_MODELS_FETCH=1` disables the cache downloads and the
runtime cache merge, not those explicit commands.
Catalog models keep their configured variants and image capabilities. A model
switch or fallback rebuilds the shared prompt with that model's delegation policy.
Project instructions, explicit system prompts and command-line rules persist
across the switch and session resume.

Known context and output limits come from configuration or catalog metadata.
Unknown limits remain unknown; the backend does not infer capacity from a model
name. The coordinator budgets messages, tools, images and reserved output before
dispatch. Successful local fixtures establish protocol behavior, not access to a
particular live account or model.

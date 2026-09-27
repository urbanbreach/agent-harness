# Connect a provider

Local HTTP fixtures verify requests, streaming, tool results, login callbacks,
device polling, refresh and credential redaction. A live Codex prompt using an
existing stored credential also passed. Fresh interactive login and other live
accounts were not exercised; see the [verification record](../architecture/backend-rewrite.md).

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

Stored credentials take precedence over environment keys and inline keys. An
expired stored OAuth credential must refresh successfully; it does not silently
fall through to another account. Credentials join the run's redaction registry
before use. Refresh retains old secrets in that registry so delayed output cannot
expose them.

## Choose a transport

Provider definitions use `type: "openai_compatible"` or `type: "anthropic_messages"`.
For OpenAI-compatible servers, `apiMode` selects the wire format:

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
Current Microsoft code sends GitHub OAuth bearer tokens directly to its CAPI
Responses and Chat endpoints; the rewrite follows that path. In automatic mode,
Claude models use the native Anthropic Messages endpoint with bearer authentication.
An explicit base URL can select another service endpoint.
See [Microsoft's CAPI service implementation](https://github.com/microsoft/vscode/blob/main/src/vs/platform/agentHost/node/shared/copilotApiService.ts).

## Startup and model limits

Catalog selection is offline. Without project configuration, it selects an
embedded provider with stored or environment credentials. Connected Codex and
Copilot providers also augment explicit configurations when absent. Existing
provider definitions, model choices and instructions remain authoritative.
Codex also reads missing, supported model entries from `HARNESS_MODELS_PATH` or
the credential directory's `models-cache.json`. Cached entries never replace
configured models. Invalid caches are ignored. Runtime
selection does not fetch or write a catalog; the connect flow and explicit model
commands perform refreshes. `HARNESS_DISABLE_MODELS_FETCH=1` selects embedded data.
Catalog models keep their configured variants and image capabilities. A model
switch or fallback selects that model's prompt family for the next request.
Project instructions, explicit system prompts and command-line rules persist
across the switch and session resume.

Known context and output limits come from configuration or catalog metadata.
Unknown limits remain unknown; the backend does not infer capacity from a model
name. The coordinator budgets messages, tools, images and reserved output before
dispatch. Successful local fixtures establish protocol behavior, not access to a
particular live account or model.

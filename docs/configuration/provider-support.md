# Provider support

Harness V1 executes through the OpenAI-compatible provider path. Larger provider catalogs are metadata/reference unless the configured provider is an implemented OpenAI-compatible transport.

## Execution path

Provider requests flow through configured provider/model ids, the coordinator, and the `harness-providers` stream interface. Deterministic tests use mock/faux providers by default; live lanes are env-gated.

## Known limits

The runtime does not implement new provider protocols in this slice. Doctor validates local configuration and credential presence but does not prove authentication because it makes no provider call.

Optional local free live targets (for example Ollama) are **deferred** as a non-CI residual path.
They are not a CI default, not part of `signoff-live`, and not required for quality gates.
Document or script them only as maintainer-opt-in dogfood.

## Fallback policy

V1 documents a minimal fallback policy: OpenAI-compatible `auto` mode may fall back from Responses API to Chat Completions when that transport path is unsupported. Model fallback by error category is documented as no-op unless an explicit `model_profile` fallback chain is configured and tested later. Failures remain visible to the operator.

## Credentials

Use config/env-backed provider credentials. Missing credentials are reported without printing secret values. Invalid credentials and rate limits require live prompt evidence because doctor stays offline.

## Model catalog refresh

The bundled model catalog is refreshed from `https://models.dev/api.json` using a five-minute cache. Harness accepts both the direct models.dev provider map and the generated catalog shape, serves a valid stale cache immediately, and refreshes stale data in the background with an atomic, mode-`0600` cache write. Set `HARNESS_DISABLE_MODELS_FETCH=1` to keep the embedded catalog only; `HARNESS_MODELS_URL` and `HARNESS_MODELS_PATH` override the source and cache location.

For the built-in `openai-codex` provider, refreshed OpenAI model metadata is merged into the configured Codex model list without replacing explicit entries. This lets newly published GPT models appear in `/model` while preserving local variants and provider settings. Unknown live entries receive conservative metadata and the existing Codex model-id reasoning policy; a provider-specific model endpoint is not required for this catalog path.

## Stable error categories

| Category | Event value | Meaning | Remediation |
|---|---|---|---|
| MissingCredentials | `missing_credentials` | No usable API key or credential reference is present. | Set `apiKey` or the configured env var. |
| InvalidCredentials | `invalid_credentials` | Provider rejects authentication. | Rotate/check credentials and endpoint. |
| RateLimited | `rate_limited` | Provider rate or quota limit. | Wait, reduce load, or change account/model. |
| ContextWindowExceeded | `context_window_exceeded` | Request exceeds model context. | Compact, shorten prompt, or pick a larger context model. |
| UnsupportedToolCall | `unsupported_tool_call` | Provider/model cannot process requested tool call shape. | Use a tool-capable model or reduce tool request. |
| MalformedStream | `malformed_stream` | Stream payload is invalid or incomplete. | Retry and keep event/support bundle evidence. |
| TransportFailure | `transport_failure` | Timeout, DNS, connection, TLS, or socket failure. | Check network/baseURL/proxy. |
| Other | `other` | Anything not classified above. | Inspect sanitized provider message and support bundle. |

## Surfacing

Provider categories are persisted in `ProviderRequestFinished.metadata.provider_error_category` with `provider_error_remediation`. Headless `prompt` failures include the serialized category plus provider message in stderr, and the TUI activity/runtime state shows the category with remediation so the operator can retry without reading raw provider payloads.

## Model fallback policy

OpenAI-compatible `auto` mode can fall back from the Responses API to Chat Completions when the configured transport reports that the Responses path is unsupported. No automatic model fallback is performed for provider error categories in V1; category failures stay visible in the event log and operator surfaces until an explicit fallback chain is added and tested.

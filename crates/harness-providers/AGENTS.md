# AGENTS: crates/harness-providers

## OVERVIEW
Provider protocol boundary crate: streaming completion trait and events, config-reachable leaf construction, OpenAI-compatible and Anthropic Messages transports, deterministic mock with request-shape digests, attachment protocol, schema compatibility families, and secret-safe cassette replay/recording for provider tests.

Read root `AGENTS.md` first. This crate separates `harness-core` from network/provider implementation details.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Provider protocol | `src/lib.rs` | `Provider`, `ProviderRouter`, `CompletionRequest`, `CompletionMessage`, `ProviderStreamEvent`, `ProviderCredentialSource`/errors, usage and metadata structs. |
| Leaf construction | `src/leaf.rs` | `build_provider`/`resolve_protocol`, `ProviderProtocol`, `ProviderLeafParams`, typed `ProviderError`. |
| OpenAI facade | `src/openai.rs` | Public re-exports only; submodule routing. |
| OpenAI config/profile | `src/openai/config.rs` | `OpenAiCompatibleProviderConfig`, `OpenAiApiMode`, `OpenAiAuthProfile`. |
| OpenAI provider | `src/openai/provider.rs` | `OpenAiCompatibleProvider`: HTTP orchestration, auth/profile handoff. |
| OpenAI transport/endpoint | `src/openai/transport.rs`, `src/openai/endpoint.rs` | `OpenAiHttpTransport`/`OpenAiHttpResponse`, Codex/Copilot endpoints. |
| OpenAI request/header | `src/openai/request.rs`, `src/openai/header.rs` | Chat Completions/Responses request mapping, header redaction. |
| OpenAI stream dispatch | `src/openai/stream.rs`, `src/openai/sse.rs` | Mode routing with Auto fallback, SSE parsing. |
| OpenAI per-mode SSE | `src/openai/stream/chat_sse.rs`, `src/openai/stream/responses_sse.rs` | Chat and Responses stream consumption. |
| OpenAI stream decode | `src/openai/stream_event.rs`, `src/openai/stream_payload.rs`, `src/openai/tool_call.rs` | Start metadata from headers, payload decoding, tool-call delta assembly. |
| OpenAI errors/tests | `src/openai/error.rs`, `src/openai/tests.rs`, `src/openai/tests/*` | Transport errors; request/media serialization, auth profiles, responses cache, tool errors, usage option, live smoke. |
| Anthropic transport | `src/anthropic.rs` | Messages request/SSE mapping and `AnthropicProvider`. |
| Mock provider | `src/mock.rs` | `MockProvider`, `request_digest` (ignores volatile context ids). |
| Attachment protocol | `src/attachment_protocol/` | `AttachmentProtocol`/capability, `serialize_attachments`. |
| Schema compatibility | `src/schema_compat.rs`, `src/schema_compat/{openai,gemini,kimi}.rs` | `ProviderSchemaFamily`, `prepare_tools_for_family`, `prepare_request_tools`. |
| Cassettes | `src/cassette.rs`, `src/cassette/{provider,transport,safety,types}.rs` | `RecordedProvider`, `RecordedOpenAiHttpTransport`, `assert_cassette_is_safe`. |
| Cassette fixtures/recorded | `tests/fixtures/cassettes/`, `tests/recorded/` | Replay fixtures and recorded integration behavior. |
| Integration tests | `tests/*` | Leaf contract, schema parity, attachment protocol, tool payload snapshots, recorded behavior. |

## PROTOCOL RULES
- `ProviderStreamEvent` is the provider-facing event stream; downstream coordinator semantics depend on stable variants.
- Store only redacted provider metadata: response/session/cache ids, usage, stop summaries, thinking summaries/digests, or signatures.
- Never persist raw provider requests, raw responses, auth headers, cookies, API keys, PEM blocks, refresh tokens, or hidden reasoning text.
- `build_provider` in `src/leaf.rs` is the only config-reachable construction boundary; keep errors protocol-typed with no silent fallback.
- Direct OpenAI, OpenAI-compatible, Codex-auth, and Copilot-auth behavior stay transport-profile decisions, not core scheduling semantics.
- `schema_compat` picks the family by provider/model and sanitizes tool schemas; native tool schema serialization must stay alias-free.
- Mock/cassette paths are test infrastructure; real transport behavior belongs in `openai/` submodules and `anthropic.rs`.
- `request_digest` must ignore volatile provider context ids while preserving meaningful request-shape changes.

## TESTS
```bash
cargo nextest run -p harness-providers
cargo nextest run -p harness-providers --test recorded_test
cargo nextest run -p harness-providers --test provider_leaf_contract_test
cargo nextest run -p harness-providers --test provider_schema_compatibility_test
cargo nextest run -p harness-providers --test provider_tool_payload_snapshot_test
cargo nextest run -p harness-providers --test attachment_protocol_test
cargo nextest run -p harness-providers --test openai_compatible_serializes_native_tool_schema_without_alias_dupes_test
cargo nextest run -p harness-providers request_serialization
cargo nextest run -p harness-providers responses_cache
cargo nextest run -p harness-providers auth_profiles
```

Run secret gates after cassette or provider metadata changes:
```bash
scripts/test-lanes.sh quality-gates
```

## ANTI-PATTERNS
- Do not let provider ids become coordinator scheduling keys; harness-owned ids drive replay/resume.
- Do not move permission, tool execution, event append, or compaction logic into providers.
- Do not add provider-specific semantics to core stream variants when optional metadata is enough.
- Do not loosen cassette secret checks to accept real credentials.
- Do not re-grow `src/openai.rs` with code that belongs in provider/request/transport/stream/tool-call helper modules.
- Do not let attachment serialization, tool payload snapshots, or cassettes carry raw provider payloads or secrets.
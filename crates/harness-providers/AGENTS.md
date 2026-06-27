# AGENTS: crates/harness-providers

## OVERVIEW
Provider boundary crate: streaming completion trait, OpenAI-compatible transport, deterministic mock provider, request-shape digests, and cassette replay/recording for provider tests.

Read root `AGENTS.md` first. This crate separates `harness-core` from network/provider implementation details.

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Provider protocol | `src/lib.rs` | `Provider`, `CompletionRequest`, `CompletionMessage`, `ProviderStreamEvent`, `ToolDef`. |
| OpenAI transport shell | `src/openai.rs` | HTTP client orchestration, auth/profile handling, API mode handoff. |
| OpenAI request/endpoint | `src/openai/request.rs`, `src/openai/endpoint.rs`, `src/openai/header.rs` | Request body construction, endpoint routing, header redaction. |
| OpenAI streaming | `src/openai/sse.rs`, `src/openai/stream_event.rs`, `src/openai/stream_payload.rs`, `src/openai/tool_call.rs` | SSE parsing, stream metadata, payload decoding, tool-call delta assembly. |
| OpenAI errors/tests | `src/openai/error.rs`, `src/openai/tests/` | Transport errors, auth profiles, request serialization, cache/SSE/tool behavior. |
| Mock provider | `src/mock.rs` | Deterministic scripted events for offline tests. |
| Cassettes | `src/cassette.rs`, `tests/fixtures/cassettes/`, `tests/recorded/` | Recorded replay/record mode and secret-safe cassette writes. |
| Provider tests | `tests/*` | Recorded behavior and OpenAI-compatible schema parity. |

## PROTOCOL RULES
- `ProviderStreamEvent` is the provider-facing event stream; downstream coordinator semantics depend on stable variants.
- Store only redacted provider metadata: response/session/cache ids, usage, stop summaries, thinking summaries, or digests.
- Never persist raw provider requests, raw responses, auth headers, cookies, API keys, PEM blocks, refresh tokens, or hidden reasoning text.
- Mock/cassette paths are test infrastructure; real transport behavior belongs in `openai.rs` and `openai/` submodules.
- OpenAI-compatible schema changes must preserve native tool schema serialization without alias dupes.
- Provider request digests should ignore volatile context ids while preserving meaningful request-shape changes.
- Direct OpenAI, OpenAI-compatible, Codex-auth, and Copilot-auth behavior should stay transport-profile decisions, not core scheduling semantics.

## TESTS
```bash
cargo test -p harness-providers
cargo test -p harness-providers --test recorded_test
cargo test -p harness-providers --test openai_compatible_serializes_native_tool_schema_without_alias_dupes_test
cargo test -p harness-providers request_serialization
cargo test -p harness-providers responses_cache
cargo test -p harness-providers auth_profiles
```

Run secret gates after cassette or provider metadata changes:
```bash
scripts/test-lanes.sh quality-gates
```

## ANTI-PATTERNS
- Do not let provider ids become coordinator scheduling keys; harness-owned ids drive replay/resume.
- Do not move permission, tool execution, event append, or compaction logic into providers.
- Do not loosen cassette secret checks to accept real credentials.
- Do not add provider-specific semantics to core stream variants when optional metadata is enough.
- Do not re-grow `src/openai.rs` with code that belongs in endpoint/request/SSE/tool-call helper modules.

# PROVIDER SOURCE GUIDE

## OVERVIEW

Score 13: more than 20 Rust files across six subdirectories, a `lib.rs` API boundary, and measured high symbol/export density make this the protocol normalization domain.

## STRUCTURE

```text
src/
|- lib.rs                 # provider-neutral request, event, router, and error contracts
|- openai/                # Chat/Responses payloads, SSE, auth profiles, transport
|- anthropic/             # Messages request/response and SSE normalization
|- cassette/              # sequential safe record/replay wrappers
|- attachment_protocol/   # capability-aware image/text serialization
|- request_budget/        # provider framing and token-cost semantics
|- schema_compat/         # provider-specific tool-schema lowering
`- mock.rs                # deterministic scripted and fixture-backed provider
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Change normalized provider API | `lib.rs` | All backends converge on `CompletionRequest` and `ProviderStreamEvent`. |
| Change OpenAI-compatible behavior | `openai/provider.rs`, `openai/request.rs`, `openai/stream.rs` | Preserve Chat, Responses, Auto, Codex, and Copilot distinctions. |
| Change Anthropic behavior | `anthropic.rs`, `anthropic/provider.rs` | Keep block ordering, tool calls, and terminal usage metadata. |
| Change recordings | `cassette.rs`, `cassette/` | Matching is sequential and persistence must pass secret checks. |
| Change tool schemas | `schema_compat.rs`, `schema_compat/` | Compatibility rules intentionally differ by provider family. |
| Change attachments | `attachment_protocol/mod.rs`, `attachment_protocol/` | Enforce MIME, capability, byte-size, dimensions, and UTF-8 constraints. |

## CONVENTIONS

- Normalize transport-specific streams into stable text, reasoning, tool-call, usage, metadata, and categorized-error events.
- Keep wire enums in `snake_case`; omit absent optional fields and retain compatibility aliases only where protocol evidence requires them.
- Make transports injectable so request serialization and stream parsing remain deterministic under tests.
- Canonicalize mock request JSON before hashing; remove volatile request IDs while retaining session-significant identity.
- Return typed provider errors with category, remediation, and optional retry delay rather than string-only failures.

## ANTI-PATTERNS

- Never persist bearer tokens, cookies, URL queries, raw requests/responses, hidden reasoning, or credential-bearing transport details.
- Do not silently choose a protocol for unsupported provider tags or malformed URLs/headers; fail closed with typed errors.
- Do not accept non-object root tool schemas or unsupported top-level combinators, tuples, cycles, or undeclared required fields.
- Do not emit completed tool calls after malformed or aborted argument streams, or lose terminal usage metadata.

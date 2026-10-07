# Provider implementation

| Work | Source |
| --- | --- |
| Normalized request/event API | `lib.rs`, `types.rs`, `error.rs` |
| HTTP dispatch and cancellation | `http.rs`, `subscription.rs`, `router.rs` |
| Chat, Responses, Anthropic encoding | `wire/` |
| Stream framing | `sse.rs` |
| Attachments and budgets | `attachment_protocol.rs`, `request_budget/` |
| Safe fixtures | `mock.rs`, `cassette.rs` |
| Claude subscription via Claude Code | `anthropic_subscription/` |

Normalize backend protocols before they leave this crate. Preserve tool-call
order, terminal usage, categorized errors, and request initiators. Automatic
protocol fallback must stop once streaming begins; authentication failures do
not authorize a different protocol. A provider with an account pool may move to
another pooled account before any output, but must block the failed account until
it signs in again (`anthropic_subscription/` does). An aborted request is never a
verdict on its account.

Validate URLs, headers, schemas, and attachments before sending. Bound incoming
frames and output accumulation. Cancellation must release the response and any
pending worker. A provider that must settle an abort overrides
`stream_completion_abortable` and ends with `Aborted`; `session_event` carries the
coordinator's model and reasoning picks, routing, compaction, rewind, and stop facts. A backend that
compacts its own session answers `manages_context`, which stands harness auto-compaction
down for that request; it reports only overflows of the harness's own re-send as
`ContextWindowExceeded`. `add_providers` hands the router providers signed in after
startup; it replaces same-named ones and keeps the rest. Unknown image/token limits
remain explicit.

Never persist wire payloads, credentials, query values, or provider reasoning.
Use local HTTP fixtures for protocol checks and nextest for execution.

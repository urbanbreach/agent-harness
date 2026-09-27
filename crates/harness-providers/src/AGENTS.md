# Provider implementation

| Work | Source |
| --- | --- |
| Normalized request/event API | `lib.rs`, `types.rs`, `error.rs` |
| HTTP dispatch and cancellation | `http.rs`, `subscription.rs`, `router.rs` |
| Chat, Responses, Anthropic encoding | `wire/` |
| Stream framing | `sse.rs` |
| Attachments and budgets | `attachment_protocol.rs`, `request_budget/` |
| Safe fixtures | `mock.rs`, `cassette.rs` |

Normalize backend protocols before they leave this crate. Preserve tool-call
order, terminal usage, categorized errors, and request initiators. Automatic
protocol fallback must stop once streaming begins; authentication failures do
not authorize a different protocol or account.

Validate URLs, headers, schemas, and attachments before sending. Bound incoming
frames and output accumulation. Cancellation must release the response and any
pending worker. Unknown image/token limits remain explicit.

Never persist wire payloads, credentials, query values, or provider reasoning.
Use local HTTP fixtures for protocol checks and nextest for execution.

# Core guide

The coordinator owns scheduling, permissions, cancellation, lifecycle, and
durable event appends. Workers submit results and intents through
`CoordinatorHandle`. Replay, projections, and readiness are side-effect-free.

| Work | Source |
| --- | --- |
| Runtime and commands | `src/coord.rs`, `src/coord/` |
| Event contracts and storage | `src/event.rs`, `src/event/`, `src/store.rs` |
| Configuration | `src/config.rs`, `src/config/` |
| Replay and projections | `src/session/`, `src/proj/`, `src/transcript_projection/` |
| Credentials and integrations | `src/auth/`, `src/integrations/` |
| Platform enforcement | `src/sandbox/`, `src/process.rs` |

Treat event IDs, sequence numbers, correlations, Serde shapes, and recorded model
selection as compatibility contracts. Preserve unknown model limits and
structured unavailable outcomes. Validate paths, symlinks, identifiers, and
credentials at boundaries. Append-only journals have exclusive writers; derived
files use private atomic writes.

Never persist provider fragments, reasoning, secrets, or raw tool arguments.
Never replay historical tools or hooks. Foreign-session imports create new
replay-only histories. Permissions are policy checks, not an OS sandbox.

Run `cargo nextest run -p harness-core` and scoped Clippy checks. Workspace lints
deny unsafe code and unchecked panic/unwrap/expect/todo paths.

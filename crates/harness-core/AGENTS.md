# HARNESS CORE KNOWLEDGE BASE

## OVERVIEW

Core runtime crate for coordination, durable events, permissions, configuration, and projections. Score 12: 463 files (+3), 100% Rust source bytes (+2), crate config (+1), `src/lib.rs` boundary (+2), and measured symbol/export density (+4); distinct ownership warrants guidance.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Public crate surface | `src/lib.rs` | Module facade and narrow crate-level re-exports |
| Coordinator runtime | `src/coord.rs`, `src/coord/` | Commands, lifecycle, scheduling, permissions, tools, compaction |
| Durable event contract | `src/event.rs`, `src/store.rs` | Schema-v1 envelopes and append/replay stores |
| Runtime configuration | `src/config.rs`, `src/config/` | JSON5 discovery, validation, model/provider registries |
| Canonical history | `src/session.rs`, `src/proj/` | Replay, provider views, resume and catalog projections |
| Integration contracts | `src/integrations/`, `src/auth/`, `src/sandbox/` | ACP/plugins, credentials, platform enforcement |
| Crate-wide behavior | `tests/` | Coordinator scenarios and replay fixtures |

## CONVENTIONS

- Treat Serde shapes, event sequences, correlation IDs, and persisted metadata as compatibility contracts.
- Use typed failures and structured outcomes with `one_line()` diagnostics at product boundaries.
- Keep durable data deterministic with B-tree collections, stable ordering, canonical digests, and explicit versions.
- Validate workspace containment, traversal, symlinks, provider IDs, and secret-bearing values before mutation.
- Runtime authority belongs to the coordinator; leaves request operations instead of appending events or mutating state.
- Public facades selectively re-export focused private modules; preserve restricted visibility boundaries.
- Workspace lints deny unsafe code and panic/unwrap/expect/todo escape hatches; narrow exceptions carry reasons.

## ANTI-PATTERNS

- Never persist provider deltas, raw provider payloads, credentials, unredacted tool arguments, or secret settings.
- Never replay historical tools, hooks, or filesystem effects while rebuilding canonical state.
- Never turn an unavailable sandbox, integration, update channel, or executable into optimistic success.
- Never bypass permission, cancellation, scheduling, lifecycle, or event-append ownership in coordinator leaves.
- Never mutate foreign sessions or VCS state during inspection/import; imports create replay-only histories.
- Never claim model capacity when limits are unknown or conservative; preserve recorded provenance.
- Keep deprecated event/config forms decode-only at explicit compatibility boundaries.

## COMMANDS

```bash
cargo build -p harness-core
cargo test -p harness-core
cargo clippy -p harness-core --all-targets
cargo fmt --check
```

## NOTES

- `rust-toolchain.toml` selects stable with rustfmt and clippy.
- Linux-only dependencies include Landlock and zbus; other platforms may intentionally report unavailable.
- Symbol reference centrality was not measured; do not infer importance from file size alone.

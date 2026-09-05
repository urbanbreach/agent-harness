# CLI IMPLEMENTATION GUIDE

## OVERVIEW

Command implementation and runtime orchestration layer; score 13 from 88 Rust files, six subdirectories, `lib.rs`, and measured symbol/export density.

## STRUCTURE

```text
src/
├── auth_cmd/  # interactive, device, browser, and stored-credential flows
├── doctor/    # local readiness checks and structured details
├── prompt/    # streaming output and correlated completion tracking
├── replay/    # history indexing and recovery summaries
├── sessions/  # listing, lineage, mutation, rewind, and export
└── tui/       # terminal workflow orchestration and event forwarding
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add or route a command | `lib.rs` | Root `Commands` dispatch and global options |
| Change prompt completion | `prompt/stream.rs` | Live/durable reconciliation and timeout state |
| Change session lifecycle | `sessions/manage.rs`, `sessions/rewind.rs` | Locking, cleanup, restart, rollback |
| Change export safety | `sessions/export/` | Credentials, readiness, routes, fail-closed scan |
| Change replay indexing | `replay/history_index.rs` | Fingerprints, locks, atomic index replacement |
| Change TUI event flow | `tui/live_events.rs`, `tui/live_intents.rs` | Lag recovery, intents, terminal events |
| Change runtime selection | `bootstrap.rs`, `runtime_catalog.rs` | Profiles, providers, models, toolsets |

## CONVENTIONS

- Command modules expose `execute_with_io`-style seams and keep output routing explicit.
- Live fragments do not advance durable sequence state; lag recovery replays from the last accepted durable sequence before resubscribing.
- Prompt completion is keyed by request/correlation identity and waits for the owning agent-turn terminal event, not provider completion alone.
- Session list/index ordering uses deterministic tie-breaks; index writes and model-selection writes use temporary files plus atomic replacement.
- Profile tools preserve configured order while deduplicating; provider/model choices come from resolved catalog metadata.
- Large orchestrators carry `// allow: SIZE_OK` reasons; do not split them mechanically without preserving their authority boundary.

## ANTI-PATTERNS

- Do not let replay infer a write-capable workspace from launcher `current_dir()` when recorded authority is absent.
- Do not finish a parent prompt on child-tool completion, provider-finished alone, or an unrelated correlation ID.
- Do not fork, clone, clean up, or rewind while source state is active or writer-locked; rollback partial filesystem restoration.
- Do not include provider reasoning deltas, raw credentials, or loaded skill bodies in session support exports.
- Do not auto-activate discovered plugins or load external code; V1 extension discovery is descriptor-only.
- Do not silently accept stale model selections, malformed credentials, unknown config keys, or unsupported index versions.

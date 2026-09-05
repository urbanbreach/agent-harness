# CORE SOURCE GUIDE

## OVERVIEW

Production implementation and public API surface. Score 13: 292 Rust files (+3), 26 immediate subdirectories (+2), 100% code bytes (+2), `lib.rs` boundary (+2), and structural scans above symbol/export thresholds (+4).

## STRUCTURE

```text
src/
├── coord/                 # coordinator-owned state machines and effects
├── config/                # public schema, discovery, validation, registries
├── session/               # canonical event replay and provider views
├── proj/                  # run, resume, background, catalog projections
├── agent/                 # provider boundary and streaming compatibility
├── integrations/          # ACP and plugin lifecycle products
├── auth/                  # credential stores and OAuth implementations
├── perm/                  # ordered rules and shell/path scanning
├── sandbox/               # availability and enforcement preparation
└── attachment_transport/  # attachment metadata and lowering
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add a crate module | `lib.rs` | Choose public, crate-only, or private visibility deliberately |
| Change coordinator behavior | `coord/handle.rs`, `coord/command_loop.rs`, `coord/state.rs` | Handle sends; loop/state own transitions |
| Change provider turns | `coord/agent_turn_phases.rs`, `coord/provider_context/` | Durable barriers plus transient overlays |
| Change compaction | `coord/session_compaction/`, `coord/compaction/` | Active paths, tool pairs, budgets, summaries |
| Change config | `config/public.rs`, `config/loader.rs`, `config/validation.rs` | Public schema is stricter than runtime shape |
| Change replay | `session/projection.rs`, `session/provider_view/`, `proj/resume_projection.rs` | Event identity is authoritative |
| Change permissions | `perm/ruleset.rs`, `perm/shell.rs`, `coord/permission.rs` | Rules and scans feed execution gates |
| Change persistence | `store.rs`, `memory.rs`, `folder_trust/store.rs` | Versioning, locking, atomic writes, redaction |

## CONVENTIONS

- Coordinator internals use restricted visibility and argument structs for orchestration-heavy calls.
- Append durable transitions with actor, stream, correlation, and causation metadata before derived state.
- Convert provider data at the explicit boundary; sanitize tool names/schemas and preserve incomplete-turn markers.
- Projection code is side-effect free and rejects mixed runs, gaps, duplicates, and invalid active paths.
- Filesystem writers validate roots, create parents, write temporary files, sync where required, then rename.
- Token accounting names each component and uses checked or saturating arithmetic; charge inputs once.
- Product probes return `Unavailable`/`Failed` evidence rather than optimistic booleans.

## ANTI-PATTERNS

- Do not expose coordinator seams merely to avoid routing through `CoordinatorHandle`.
- Do not separate a tool result from its originating call during projection or compaction cuts.
- Do not let retry failures mask the original provider error or exceed the bounded retry.
- Do not let static denies be overridden by grants or unscanned path-like tokens fail open.
- Do not activate plugin code during discovery/install; lifecycle permission precedes activation.
- Do not replace persisted semantic request fields with current configuration during resume.
- Do not add production flow through legacy multi-turn streaming or legacy session adapters.

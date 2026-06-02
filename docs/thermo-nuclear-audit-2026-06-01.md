# Thermo-nuclear code quality audit: 2026-06-01

## Scope

This audit was performed before the behavior-preserving refactors in
`crates/harness-core/src/coord.rs` and
`crates/harness-tui/src/app/session_projection.rs`. It covered the requested
whole-repository areas: architecture and module boundaries, files over roughly
1,000 lines, duplicated logic, spaghetti branching and scattered feature checks,
unnecessary wrappers or abstractions, type boundary issues, and places where
complexity can be deleted rather than moved.

The working tree already contained unrelated changes before this audit started.
The implementation slice below is therefore scoped to the coordinator append
context helpers and the TUI permission-projection duplication cleanup. It
deliberately avoids modifying the pre-existing config, model, README, and PRD
changes.

## Audit findings and plan disposition

| Finding | Evidence | Disposition |
| --- | --- | --- |
| Coordinator event append scaffolding repeats `EventContext::new`, correlation fallback, and `stream_key` construction across tool-call, permission, edit, and artifact events. | `crates/harness-core/src/coord.rs` append-helper cluster around coordinator-owned event appends. | **Completed.** Extracted private context helpers and added characterization coverage for preserved correlation fallbacks and stream keys. |
| `crates/harness-tui/src/ui_transcript.rs` is a giant presentation module, roughly 13.9k lines. | Large-file scan. | **Deferred.** Meaningful decomposition would be a broad TUI rendering architecture project requiring snapshot churn and visual evidence; not safe to mix with this behavior-preserving core refactor. |
| `crates/harness-tui/src/lib_tests.rs` is a giant test module, roughly 10.4k lines. | Large-file scan. | **Deferred.** Splitting tests is behavior-neutral but high-churn and unrelated to the selected core simplification. It should be done as its own test-organization slice. |
| `crates/harness-core/src/coord.rs` remains a giant coordinator module, roughly 10.3k lines. | Large-file scan and coordinator guidance in `crates/harness-core/AGENTS.md`. | **Partially addressed.** The implemented append-context cleanup reduces one repeated local pattern. Larger coordinator decomposition must preserve single-authority invariants and should be staged separately. |
| `crates/harness-core/src/config.rs` is a large config boundary module, roughly 5.3k lines. | Large-file scan. | **Deferred.** Public config keys require synchronized docs, generated schemas, and tests. Existing dirty config changes also fail clippy independently, so this audit slice does not touch it. |
| `crates/harness-providers/src/openai.rs` is a large provider transport module, roughly 4.0k lines. | Large-file scan. | **Deferred.** Provider transport changes risk live/cassette behavior and should be audited with provider-specific tests and cassettes. |
| TUI permission projection duplicates permission-entry mutation across activity-level and tool-call-level permission flows. | `crates/harness-tui/src/app/session_projection.rs` had duplicate `PermissionEntry` resolution field updates in activity-level and tool-call-level loops. | **Completed.** Moved resolution mutation into `PermissionEntry::mark_resolved`, reused it from both projection paths, and added/extended regression coverage for activity-level and tool-call-level permission resolution. |

## Implemented finite plan

The finite implementation plan for this pass is complete:

1. Lock existing coordinator event metadata behavior with a characterization test.
2. Extract private helpers that centralize event context creation while keeping
   coordinator append authority unchanged.
3. Cover tool-call, permission, edit, permission-grant, and artifact append
   paths for correlation fallback and `stream_key` preservation.
4. Centralize TUI permission resolution mutation in `PermissionEntry` so
   activity-level and tool-call-level projection paths share one state update.
5. Verify formatting, focused tests, full `harness-core`, TUI permission
   projection tests, build/checks, LSP diagnostics, and a CLI scenario surface.

## Documentation impact

No `AGENTS.md` guidance or public docs changed as a result of the code refactor:
no public config keys, event variants, replay semantics, native tool surface,
test lane behavior, simulation invariant, provider catalog, or starter config
default changed. This audit document is the only required documentation artifact
for the thermo-nuclear plan itself.

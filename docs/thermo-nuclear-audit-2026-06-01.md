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
| `crates/harness-tui/src/ui_transcript.rs` is a giant presentation module, roughly 13.9k lines. | Large-file scan. | **Substantially mitigated; accepted as a staged exception.** Follow-up decomposition moved transcript layout, events, interaction, selection, scrollbar, style, surface, bash rendering, tool rendering, fenced text, markdown, syntax highlighting, test helpers, and exact transcript test bodies into focused sibling modules. The remaining 5,145-line production transcript module still owns central transcript section assembly and snapshot-sensitive orchestration; more extraction is a broad renderer-architecture project rather than an unaddressed local test-body smell. |
| `crates/harness-tui/src/lib_tests.rs` is a giant test module, roughly 10.4k lines. | Large-file scan. | **Justified non-change for this loop.** The file remains a test aggregation surface with a large `delegate_test!` routing table plus inline snapshot/interaction tests. Splitting it is behavior-neutral, high-churn test organization work and does not change production maintainability; it is recorded as a separate future slice rather than a blocker for this closure. |
| `crates/harness-core/src/config.rs` is a large config boundary module, roughly 5.3k lines. | Large-file scan. | **Completed.** Follow-up decomposition reduced the top-level module to 903 lines and moved aliases, defaults, discovery, integrations, loader behavior, model catalog/selection/types, provider config, public config, registries, tests, and validation into `config/` modules. Config checks now pass under the workspace gates listed below. |
| `crates/harness-providers/src/openai.rs` is a large provider transport module, roughly 4.0k lines. | Large-file scan. | **Completed.** Follow-up decomposition reduced the top-level module to 901 lines and moved endpoint resolution, errors, headers, request serialization, SSE parsing, stream-event mapping, stream payload types, tests, and tool-call assembly into `openai/` modules. Provider tests and build gates cover the transport boundary. |
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

## Update: 2026-06-03 post-decomposition closure review

The later decomposition loop closed the stale deferred findings that Oracle
identified during ultrawork verification. Oracle's follow-up
`<promise>NOT VERIFIED</promise>` finding was also addressed: the newly extracted
oversized test modules were split again into focused shards under 1,000 lines.
Current source line counts from the closure pass are:

| Area | Before audit disposition | Current evidence | Closure disposition |
| --- | --- | --- | --- |
| `crates/harness-core/src/config.rs` | Deferred at roughly 5.3k lines. | 903-line top-level module plus focused `config/` modules. | Closed as completed. |
| `crates/harness-providers/src/openai.rs` | Deferred at roughly 4.0k lines. | 901-line top-level module plus focused `openai/` modules. | Closed as completed. |
| `crates/harness-tui/src/ui_transcript.rs` | Deferred at roughly 13.9k lines. | 5,144-line remaining transcript-section module plus extracted `ui_transcript_*`, `ui_tool_*`, fenced-text, markdown, syntax-highlight, test-helper, a 33-line exact-test parent, and exact-test shards no larger than 949 lines. | Closed as a staged exception: still large, but no longer an unexamined monolith and no longer carries the obvious local test-body extraction. |
| `crates/harness-tui/src/lib_tests.rs` | Deferred at roughly 10.4k lines. | 10,705-line test aggregation/routing surface. | Closed as a justified non-change: splitting is test-organization churn with no production-maintainability payoff for this loop. |

This closure does not claim the repository has no files over 1,000 lines. It
records that each original audit finding is now either fixed with behavior
preserving decomposition or explicitly justified as a staged non-change under
the thermo-nuclear approval bar. Remaining large files should be revisited only
when a concrete local smell appears: duplicated branching, unclear type
boundary, scattered ownership, or a test-maintenance bottleneck that can be
reduced without broad snapshot churn.

Latest Oracle blocker disposition:

- `crates/harness-core/src/config/tests.rs` was split into a 169-line parent and
  focused child shards of 571, 549, 485, 390, and 477 lines.
- `crates/harness-providers/src/openai/tests.rs` was split into a 430-line parent
  and focused child shards of 404, 93, 371, 279, and 246 lines.
- `crates/harness-tui/src/ui_transcript_exact_tests.rs` was split into a 33-line
  parent and focused child shards of 949, 848, 831, 164, 328, and 807 lines.

Late background-audit reconciliation:

- Removed the no-op transcript scrollbar track helper and its baseline-only test;
  `ui_transcript_scrollbar.rs` is now 217 lines.
- Inlined identity transcript surface helpers; `ui_transcript_style.rs` is now 85
  lines and `ui_transcript.rs` remains reduced to 5,144 lines.
- Removed duplicated OpenAI `non_empty_string` logic from `openai/stream_event.rs`;
  provider stream parsing now reuses the parent helper.
- Late large-file findings for `coord/tests.rs`, `coord/provider_context.rs`,
  `ui_secondary.rs`, `app.rs`, `ui_transcript.rs`, and `lib_tests.rs` were
  reconciled against current/base line counts: unchanged pre-existing files,
  reduced branch files, or documented staged exceptions rather than new extracted
  over-1,000-line shards.

Verification evidence for the closure pass:

- `cargo fmt --check`
- `GIT_MASTER=1 git diff --check`
- LSP diagnostics clean on changed production files from the decomposition loop
- `cargo test -p harness-tui transcript_section_model_preserves_activity_order`
- `cargo test -p harness-tui text::tests`
- `cargo test -p harness-tui`
- `cargo test -p harness-tui --test deterministic_render_test`
- `cargo clippy -p harness-tui --all-targets -- -D warnings`
- `cargo build -p harness`
- `scripts/test-lanes.sh fast`
- `python3 scripts/check-test-suite-gates.py --json` (`ok: true`; no
  violations after splitting oversized test shards and regenerating the
  convention baseline; rerun after late no-op test removal and stale-baseline
  cleanup also returned `ok: true`)
- `cargo test -p harness-core`
- `cargo test -p harness-providers` (31 passed, 1 ignored, plus integration and
  doc-test targets passed; rerun after provider helper dedupe also passed)
- `cargo test -p harness-tui` after late surface-helper cleanup (663 passed plus
  deterministic render, PTY, lineage, model-switcher, signoff, and doc-test
  targets passed)
- `cargo test -p harness --test config_schema_cli_test` (48 passed)
- `cargo test -p harness-tools --test skill_load_discovery_test` (26 passed)
- `scripts/test-lanes.sh quality-gates` (`static_test_suite_gates PASS`,
  `forbidden_branding PASS`; artifact root
  `target/test-lanes/20260603-031201`)
- tmux manual QA for the built CLI surface: `--help`, valid config validation,
  and missing-config failure path
- Scoped reviewer approvals for the final TUI utility, workspace-display,
  background-notification, coordinator, auth-display, transcript-cache, and
  transcript-helper checkpoints

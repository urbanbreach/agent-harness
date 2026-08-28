# Engine migration

Migration starts from baseline commit `060ee1fd` and is intentionally serial: limits and budget,
then canonical session semantics, then compaction, consumers, bounded indexing, and deletion.

| Stage | Baseline | Target | Migration disposition |
|---|---|---|---|
| model limits/budget | distributed policy/context calculations | one typed provenance record and budget | consolidate before consumer changes |
| durable session | overlapping event/projection interpretations | one canonical session and typed identities | migrate readers before writers |
| compaction | legacy and active variants coexist | one atomic, restart-safe pipeline | disable old active path, then Delete it |
| consumers | CLI/TUI/tools independently reconstruct history | canonical replay-derived views | Move each consumer after equivalence proof |
| cleanup | SIZE_OK and overlap counts are baseline facts | net-negative overlap with evidence | Delete only after green owner evidence |

Legacy logs remain readable for list, search, inspect, replay, and export. Their mutation paths
fail as read-only until a separate migration is explicitly implemented. Each stage records an
`engine-metrics-v1` artifact against the same baseline so reductions are comparable rather than
silently rebased.

## M02 model-limit consolidation

M02 replaces the catalog, profile selection, recorded runtime context, CLI list, and TUI metadata
copies with `ResolvedModelLimits`. Generated and provider-discovered entries retain source and
verification-date provenance; explicit configuration remains distinguishable. Strict boundary
validation rejects partial, zero, and impossible triples, while entirely absent custom limits are
recorded as unknown. No request-budget or compaction policy changed in this milestone.

## Baseline lane reconciliation

The pre-migration baseline was rechecked at `ccdefb5c65693880a05fbfc63f7a30094043d552`
before engine behavior changed. The exact deterministic identities and dispositions are:

| Identity | Observed root cause | Disposition |
|---|---|---|
| `harness::replay_sessions_cli_test part_12_sessions_export_fails_closed_for_missing_session_test::sessions_export_cli_fails_closed_for_missing_session_dir` | The previously reported assertion failure did not reproduce: it passed in the combined 86-test RED run. The test only proved a generic nonzero status, empty stdout, and an error fragment, however; it did not name the exact exit code, require one diagnostic line, or prove that `--output` stayed absent. | Production export behavior was left unchanged. The in-process `CliHarness`/`CliIo` test now requires exit code 1, empty stdout, one `failed to read session directory` diagnostic, and no output bundle. |
| `harness::bootstrap_profiles_test shipped_v1_full_composed_prompt_snapshots_match_source` | All four `v1_composed_prompts/{default,explore,general,librarian}.txt` files still embedded root guidance from before `baf1443b`: stale generated commit metadata and volatile code-reference counts remained after the intentional root `AGENTS.md` cleanup. | Regenerated only those four composed snapshots. Semantic review confirmed identical prompt-family/runtime prose and only the intentional root-guidance deletions/table-shape change. |
| `python3 scripts/check-test-suite-gates.py --gate conventions --json` | 42 live test bodies lacked the gate's exact `// arrange`, `// act`, and `// assert` phases, including bodies that used unrecognized capitalized Given/When/Then prose. | Repaired every reported body with real phase boundaries or a focused result collection; the baseline JSON was not edited and the JSON gate now reports zero violations. |

The 42 convention findings were distributed exactly as follows: 1 in
`harness-core/src/agent.rs`, 4 in `harness-core/src/store/tests.rs`, 2 in
`harness-providers/src/openai/sse.rs`, 1 in `harness-tools/src/fs_grep.rs`, and 34 in
`harness-tui` (18 of those in `app/tests/permission_modal_tests_part3_test.rs`).

## G008-G012 shipped boundary

The final migration tail uses accepted baseline `2f0b2a9a` and preserves all earlier invariants.

- G008 is recorded by commit `8c375d31`: one composed `CanonicalSessionProjection` facade serves
  settled CLI and TUI reads. Its focused reducers remain pure and separate; live TUI fragments stay
  presentation-only.
- G009 is recorded by commit `9c3274b5`: `.session-history-index-v1.json` rows update after
  successful durable commits, pagination uses a run-directory tie-break, and the warm open seam is
  observable. Continuation still validates source history.
- G010 retains old compaction and stream variants as compatibility-only decode input. Their matching
  remains inside the read-only `session::legacy` boundary; no legacy writer is introduced.
- G011 keeps the coordinator as the event append, scheduling, permission, and lifecycle authority
  while neutral crash-tail recovery and identity helpers use canonical namespaces.
- G012 updates the public architecture documents and records current metrics/report evidence. Its
  final lane, dogfood, PTY/TUI, and independent-review entries are status **PENDING** until they run
  against the final source SHA; earlier artifacts are historical context, not reused final proof.

Removed production paths are the checkpoint artifact writer/loader/copy flow, detached planning
and summary builders, duplicate context constructors, and orphan checkpoint tests. Compatibility
modules may decode old bytes; they cannot append legacy events or supply a second source of truth.

The final feature matrix classifies canonical reads, Compaction V2, and the advisory history index
as supported; live presentation fragments as experimental; shipped old event decoding as
compatibility-only; and obsolete checkpoint/copy/context paths as removed. The `EventV1` count stays
at 39 for shipped journal compatibility rather than being silently rebased.

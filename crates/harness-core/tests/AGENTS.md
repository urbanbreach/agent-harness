# AGENTS: crates/harness-core/tests

## OVERVIEW
Integration/owner test surface for harness-core: 40 top-level `*_test.rs` binaries, numbered fan-in suites, and shared fixtures in `common/`.

Read `../AGENTS.md` first for crate invariants, config contract, and deep-scope pointers. This directory proves runtime behavior; it owns no production logic.

## STRUCTURE
```text
tests/
├── coord_test.rs              # fan-in binary: includes common/coord_fixtures.rs + coord/01..23 (+ lettered variants)
├── coord/                      # 27 numbered coordinator suites (01..23 + 07b/07c/12b/22b; turn loop, permission, compaction, resume, retry, titles, hooks)
│   └── common/                 # resume acceptance fixture
├── common/                     # shared fixture modules included by fan-in binaries
│   └── coord_fixtures/         # provider tool fixture helpers
├── conversation_projection/    # provider-boundary projection suites (fan-in via *_test.rs)
├── transcript_projection/      # transcript projection suites
├── native_metadata_replay/     # native tool artifact replay suites
├── resume_plan/                # resume plan reconstruction suites
├── session_lineage_materialization/ # lineage materialization suites
├── perf/                       # perf suites (fan-in via perf_test.rs)
└── *_test.rs                   # 40 top-level owner and fan-in binaries
```

## OWNER GROUPS
| Group | Binaries | Notes |
|-------|----------|-------|
| Coordinator fan-in | `coord_test.rs` + `coord/` | `spawn_coordinator`/`CoordinatorHandle` acceptance: turn loop, permissions, compaction, resume, retries, titles, hooks. Uses `common/coord_fixtures.rs`. |
| Common fixtures | `common/` | coord, conversation/native-metadata/resume-plan/session-lineage/transcript fixtures. Extend, do not copy. |
| Replay/projection | `conversation_projection_test`, `transcript_projection_test`, `native_metadata_replay_test`, `resume_plan_test`, `session_lineage_materialization_test`, `replay_preserves_batch_and_child_task_metadata...`, `recorded_runtime_context_meta_test`, `task_schedule_lineage_test` | Replay-derived; must not execute providers, tools, hooks, MCP, network, or the CLI. |
| Permission/auth | `permission_policy_supports_native_tool_permission_kinds_test`, `permission_visibility_matches_execution_test`, `batch_inherits_nested_tool_permissions_without_bypass_test`, `coord_auth_test`, `coord_ast_grep_auth_test`, `coord_auth_apply_patch_permission_test`, `browser_oidc_test`, `poc_candidate*` (3) | Permission ordering/visibility, auth flows, OAuth, security PoCs. |
| Integrations | `integrations_lifecycle_test`, `integrations_lifecycle_part2_test`, `integrations_matrix_test`, `acp_lifecycle_test`, `extension_manifest_test`, `plugin_runtime_contract_test`, `workspace_hub_absence_test`, `mcp_config_test`, `mcp_scope_test`, `model_variant_resolution_test`, `attachment_transport_test` | Plugin lifecycle, ACP, MCP, extension descriptors, attachment transport. |
| Perf | `perf_test.rs` + `perf/` | Budgeted replay/resume-plan projections; env-tunable budgets. |
| Workspace/sandbox/misc | `workspace_vcs_trust_test`, `sandbox_network_test`, `memory_queue_compaction_test`, `file_tag_test`, `edit_attribution_test`, `foreign_session_test`, `leaf_contracts_test`, `agent_profile_toolsets...`, `function_name_mapping...` | Worktree trust, sandbox confinement, memory/queue, file tags, attribution, foreign session import. |

## CONVENTIONS
- Fan-in binaries `include!` their suites and fixtures; add a numbered suite under `coord/` and wire it in `coord_test.rs`.
- Shared helpers live in `common/` and are `include!`d; never duplicate a fixture inside a standalone binary.
- Replay/projection tests assert on replay-derived data only; no provider, tool, hook, MCP, network, or CLI execution.
- Perf tests read budgets from env (`HARNESS_PERF_*`); keep them bounded and CI-safe.
- Standalone `*_test.rs` files are independent nextest targets; run them by name to target one owner.

## COMMANDS
```bash
cargo nextest run -p harness-core                        # all owner binaries + unit tests
cargo nextest run -p harness-core --test coord_test      # coordinator fan-in
cargo nextest run -p harness-core --test resume_plan_test
cargo nextest run -p harness-core --test conversation_projection_test
cargo nextest run -p harness-core --test permission_policy_supports_native_tool_permission_kinds_test
cargo nextest run -p harness-core --test integrations_lifecycle_test
cargo nextest run -p harness-core --test perf_test
```

## ANTI-PATTERNS
- Do not put runtime logic in fixtures; fixtures stay deterministic and side-effect free.
- Do not make a standalone binary re-implement a `common/` fixture; extend and `include!` it.
- Do not make replay/projection tests execute providers, tools, hooks, MCP, network, or the CLI.
- Do not add a numbered `coord/` suite without wiring it into `coord_test.rs`.
- Do not assert on prompt prose; assert routing decisions and structural tokens.

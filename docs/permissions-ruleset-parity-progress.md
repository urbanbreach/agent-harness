# Harness Permissions Parity — Progress Ledger

**PRD:** [`PERMISSIONS_RULESET_PARITY_PRD.md`](../PERMISSIONS_RULESET_PARITY_PRD.md)  
**Date:** 2026-07-16  
**Harness tree:** `inspirations/harness`  
**Harness git:** `b1fc8113948b518835c2a39ece49553cffe9b30c`

## Phase status

| Phase | Status |
|-------|--------|
| P0 Inventory & golden fixtures | complete |
| P1 Permission ruleset + evaluate | complete |
| P2 Deny hides tools | complete |
| P3 Agent default permissions | complete |
| P4 Task + child derivation | complete |
| P5 Bash / shell ergonomics | complete |
| P6 Tool schema & description | complete |
| P7 Error surfaces | complete |
| P8 Docs, dogfood, full gates | complete |

## Harness reference

```text
tree: inspirations/harness
git: b1fc8113948b518835c2a39ece49553cffe9b30c
files:
  - packages/src/permission/index.ts
  - packages/src/agent/agent.ts
  - packages/src/agent/subagent-permissions.ts
  - packages/src/tool/registry.ts
  - packages/src/tool/task.ts
  - packages/src/tool/shell.ts
summary: evaluate last-match wins default ask; disabled when catch-all deny;
  explore allows bash/webfetch/websearch; plan partial edit allow; task description
  filters denied agents; child spawn merges deriveSubagentSessionPermission.
```

## Intentional divergences

| Item | Rationale |
|------|-----------|
| Explore uses explicit denies (not `*`:deny + allow-list) | ProfilePermissions lacks read/glob/grep/list scalars; catch-all deny would hide discovery tools via disabled() |
| external_directory grant/message-level ask | Paths outside workspace ask via `external_directory`; full desktop ask UI deferred |
| V2 SQLite always-allow | Prefer event-sourced session grants (PRD §8) |
| Plan shell=ask + read-only bash guard | Stricter than OC plan base `*` allow for bash |
| Category routes `task=deny` | Harness category routing profiles deny recursive task by default |
| Plan edit path | `.agent-harness/plans/*` (not `.opencode/plans/*`) |

## Deferred (§8)

| Item | Disposition |
|------|-------------|
| V2 SQLite always-allow | defer |
| doom_loop exact heuristics | implement (session streak ask shipped) |
| MCP tool permission naming | defer |
| Desktop permission UI | reject |
| Full non-permission tool behavior | defer |
| First-class ExternalDirectory PermissionKind + ask UI | implement (kind + grants shipped; chrome deferred) |

## P5 re-open + close (2026-07-16)

| Field | Evidence |
|-------|----------|
| Status before | RED: `validate_bash_command_allows_shell_globs_and_dev_null_redirect` failed on `2>/dev/null` |
| RED log | `target/harness-permissions-parity/red/p5-bash-red.txt` |
| Root cause | `path_validation.rs` treated `/dev/null` as workspace escape and hard-blocked shell globs |
| Fix | Safe device skip + glob prefix-only containment; external escapes still denied |
| GREEN | shell_safety suite 37/37; bash.txt PASS |
| Files | `crates/harness-tools/src/shell_safety/path_validation.rs`, adjacent escape assertion |

## Evidence commands (exit codes) — post-final-change 2026-07-16

```text
cargo nextest run -p harness-core  → 0 (756 passed)
cargo nextest run -p harness-tools  → 0 (365 passed)
cargo nextest run -p harness-providers  → 0 (59 passed)
cargo nextest run -p harness --test config_docs_reference_test  → 0 (19 passed)
cargo nextest run -p harness --test bootstrap_profiles_test  → 0 (40 passed)
cargo nextest run -p harness-core --test permission_ruleset_parity_inventory_test  → 0 (12/12)
cargo test -p harness-tools --lib shell_safety::tests  → 0 (37/37)
scripts/test-lanes.sh fast  → 0  (artifact target/test-lanes/20260716-165248)
scripts/test-lanes.sh quality-gates  → 0  (artifact target/test-lanes/20260716-165610)
scripts/test-lanes.sh all-deterministic  → 0  (artifact target/test-lanes/20260716-165613; PASS=19 FAIL=0)
```

Log: `target/harness-permissions-parity/dogfood/section9-package-nextest.txt`

## §11 checklist (evidence links)

### Behavior
- [x] evaluate + merge — `perm/ruleset.rs` + inventory/ruleset unit tests
- [x] catch-all deny omitted from provider lists — `provider_visible_tool_ids` + `p2_build_provider_tool_defs_*`
- [x] partial allows keep tools visible — plan path allow inventory + bootstrap export test
- [x] shipped agent defaults — `config/public/agents.rs` + bootstrap profile tests
- [x] task description filters denied agents — bootstrap `task_tool_description_*`
- [x] child permission derivation — `run_lifecycle.rs` + `derive_subagent_session_permission`
- [x] bash globs/`/dev/null` — shell_safety tests + dogfood/bash.txt
- [x] external_directory-equivalent — path_validation + oc_parity external_directory tests
- [x] tool schemas/descriptions — native_execution_surface + native_tool_parity_matrix
- [x] error recovery messages — `permission_policy_denied_response_message` contract test

### Quality
- [x] P0–P8 complete with evidence (this ledger)
- [x] no inspirations/ modifications
- [x] no tests deleted solely to pass
- [x] fast exit 0
- [x] quality-gates exit 0
- [x] all-deterministic exit 0
- [x] docs: permissions, config, native-tool-catalog, agents
- [x] example config ruleset-compatible permission comments
- [x] dogfood scenarios 1–4 recorded (test outputs)

### Honesty
- [x] divergences table
- [x] deferred §8 dispositioned
- [x] §12 certificate filled below

## Dogfood artifacts

| Scenario (§10) | Path |
|----------------|------|
| Explore/plan/task filter | `target/harness-permissions-parity/dogfood/explore-plan-task.txt` |
| Bash globs + `/dev/null` | `target/harness-permissions-parity/dogfood/bash.txt` |
| Deny-hide inventory | `target/harness-permissions-parity/dogfood/deny-hide.txt` |
| External directory | `target/harness-permissions-parity/dogfood/external-directory.txt` |
| Child derive | `target/harness-permissions-parity/dogfood/child-derive.txt` |
| P2/P6/P7 contracts | `target/harness-permissions-parity/dogfood/contract-p2-p6-p7.txt` |
| §9 package nextest | `target/harness-permissions-parity/dogfood/section9-package-nextest.txt` |
| Fast | `target/harness-permissions-parity/dogfood/lane-fast.txt` |
| Quality-gates | `target/harness-permissions-parity/dogfood/lane-quality-gates.txt` |
| All-deterministic | `target/harness-permissions-parity/dogfood/lane-all-deterministic.txt` |

Dogfood form: unit/bootstrap/test outputs (PRD §10 allows “event logs **or test outputs**”).

## Primary harness files

- `crates/harness-core/src/perm/ruleset.rs`
- `crates/harness-core/src/perm.rs` (`allow_all`, tool→permission mapping)
- `crates/harness-core/src/agent/provider_boundary.rs`
- `crates/harness-core/src/config/public/agents.rs`
- `crates/harness-core/src/coord/run_lifecycle.rs`
- `crates/harness-core/src/coord/permission.rs`
- `crates/harness-core/src/coord/question.rs` (ask_timeout for question waits)
- `crates/harness-tools/src/shell_safety/path_validation.rs` (P5)
- `crates/harness-tools/src/tool_catalog.rs`
- `docs/permissions.md`, `docs/config.md`, `docs/native-tool-catalog.md`
- fixtures + inventory test
- `crates/harness/tests/bootstrap_profiles/permission_ruleset_export_test.rs`

## Completion certificate — Harness permissions parity

Date (ISO): 2026-07-16  
Implementer session / agent: Sisyphus (ultrawork closeout)

### Declaration
I certify that:
1. inspirations/harness was re-read for every phase and citations are in the ledger.
2. All phases P0–P8 are complete with command exit codes recorded.
3. All §11 checkboxes are true (see §11 checklist above). Deferred items are dispositioned in §8.
4. The following commands were run after the final change and exited 0:
   - scripts/test-lanes.sh fast → exit 0 (artifact target/test-lanes/20260716-165248)
   - scripts/test-lanes.sh quality-gates → exit 0 (artifact target/test-lanes/20260716-165610)
   - scripts/test-lanes.sh all-deterministic → exit 0 (artifact target/test-lanes/20260716-165613; PASS=19 FAIL=0)
   - PRD §9 package suite → exit 0 (log target/harness-permissions-parity/dogfood/section9-package-nextest.txt)
5. Dogfood artifact paths:
   - Explore: target/harness-permissions-parity/dogfood/explore-plan-task.txt
   - Bash: target/harness-permissions-parity/dogfood/bash.txt
   - Plan: target/harness-permissions-parity/dogfood/explore-plan-task.txt
   - Deny-hide: target/harness-permissions-parity/dogfood/deny-hide.txt
6. Intentional divergences: table above (explore explicit denies; plan shell ask; category task deny; plan path; V2 SQLite deferred).
7. I have not weakened tests or edited inspirations/ to force a pass.

Signed: Sisyphus / ultrawork 2026-07-16

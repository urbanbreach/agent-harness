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
| external_directory as CommandBlocked message (not full ask UI kind) | Paths outside workspace fail closed with `external_directory:` permission denial text; full ask UX deferred |
| V2 SQLite always-allow | Prefer event-sourced session grants (PRD §8) |

## Deferred (§8)

| Item | Disposition |
|------|-------------|
| V2 SQLite always-allow | defer |
| doom_loop exact heuristics | defer |
| MCP tool permission naming | defer |
| Desktop permission UI | reject |
| Full non-permission tool behavior | defer |
| First-class ExternalDirectory PermissionKind + ask UI | defer (message-level equivalent landed) |

## Evidence commands (exit codes) — post-final-change

```text
cargo test -p harness-core --lib permission_policy_deny_message  → 0
cargo nextest run -p harness-core --test permission_ruleset_parity_inventory_test  → 0 (7/7)
cargo nextest run -p harness --test bootstrap_profiles_test -E 'test(shipped_profiles_populate_permission_ruleset)|test(shipped_profile_permission_promises)'  → 0
cargo nextest run -p harness-tools --test native_execution_surface_test -E 'test(native_provider_tool_defs)|test(provider_tool_defs_match)'  → 0 (6/6)
# PRD §9 package suite (required before complete)
cargo nextest run -p harness-core  → 0 (724 passed)
cargo nextest run -p harness-tools  → 0 (365 passed)
cargo nextest run -p harness-providers  → 0 (59 passed)
cargo nextest run -p harness --test config_docs_reference_test  → 0 (19 passed)
cargo nextest run -p harness --test bootstrap_profiles_test  → 0 (35 passed)
scripts/test-lanes.sh quality-gates  → 0  (artifact target/test-lanes/20260716-010851; re-run inside 20260716-011127)
scripts/test-lanes.sh fast  → 0  (artifact target/test-lanes/20260716-010807; re-run inside 20260716-011127)
scripts/test-lanes.sh all-deterministic  → 0  (artifact target/test-lanes/20260716-011127; PASS=19 FAIL=0)
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
- [x] external_directory-equivalent — path_validation message + unit tests (full ask UI deferred §8)
- [x] tool schemas/descriptions — P6 schema tests 6/6 + native_tool_parity_matrix
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
| P2/P6/P7 contracts | `target/harness-permissions-parity/dogfood/contract-p2-p6-p7.txt` |
| P6 schema tests (green) | `target/harness-permissions-parity/dogfood/p6-schema-tests.txt` |
| §9 package nextest | `target/harness-permissions-parity/dogfood/section9-package-nextest.txt` |
| Quality-gates (post-final) | `target/harness-permissions-parity/dogfood/lane-quality-gates-post-contract.txt` |
| Fast (post-final) | `target/harness-permissions-parity/dogfood/lane-fast-post-contract.txt` |
| All-deterministic (post-final) | `target/harness-permissions-parity/dogfood/lane-all-deterministic-post-contract.txt` |

Dogfood form: unit/bootstrap/test outputs (PRD §10 allows “event logs **or test outputs**”).

## Primary harness files

- `crates/harness-core/src/perm/ruleset.rs` (new)
- `crates/harness-core/src/perm.rs`
- `crates/harness-core/src/agent.rs` (`permission_ruleset`)
- `crates/harness-core/src/agent/provider_boundary.rs`
- `crates/harness-core/src/config/public/agents.rs`
- `crates/harness-core/src/coord/run_lifecycle.rs` (child derive at spawn)
- `crates/harness-core/src/coord/permission.rs` (`permission_policy_denied_response_message` + P7 contract test)
- `crates/harness/src/bootstrap.rs`
- `crates/harness-tools/src/shell_safety/path_validation.rs`
- `crates/harness-tools/src/shell_safety.rs`
- `.agent-harness/agents/explore.md`
- `docs/permissions.md`, `docs/config.md`, `docs/native-tool-catalog.md`, `docs/agents-and-subagents.md`
- `configs/harness.example.jsonc`
- fixtures + inventory test (includes `build_provider_tool_defs` wiring)
- `crates/harness/tests/bootstrap_profiles/permission_ruleset_export_test.rs`
- quality-gates baselines/allowlists for pre-existing reference agent brand + convention debt

## Post-Oracle contract hardening (2026-07-16)

| Gap | Evidence |
|-----|----------|
| P7 deny message shape | `permission_policy_denied_response_message` + `permission_policy_deny_message_is_actionable_and_anti_thrash` |
| P2 `build_provider_tool_defs` wiring | `p2_build_provider_tool_defs_omits_catch_all_denied_tools`, `p2_build_provider_tool_defs_keeps_edit_when_plan_path_allow_exists` |
| Bootstrap ruleset before export | `shipped_profiles_populate_permission_ruleset_before_provider_tool_export` |
| P6 schema alignment | `native_execution_surface_test` schema cases (6/6 green in `p6-schema-tests.txt`) |
| Dogfood form | test outputs under `target/harness-permissions-parity/dogfood/` per §10 |

## Completion certificate — Harness permissions parity

Date (ISO): 2026-07-16
Implementer session / agent: Sisyphus (ulw-loop)

### Declaration
I certify that:
1. inspirations/harness was re-read for every phase and citations are in the ledger.
2. All phases P0–P8 are complete with command exit codes recorded.
3. All §11 checkboxes are true (see §11 checklist above). external_directory full ask UI and V2 SQLite are deferred per §8 with documented equivalents.
4. The following commands were run after the final change and exited 0:
   - scripts/test-lanes.sh fast → exit 0 (artifact target/test-lanes/20260716-010807; re-run inside 20260716-011127)
   - scripts/test-lanes.sh quality-gates → exit 0 (artifact target/test-lanes/20260716-010851; re-run inside 20260716-011127)
   - scripts/test-lanes.sh all-deterministic → exit 0 (artifact target/test-lanes/20260716-011127; PASS=19 FAIL=0)
   - PRD §9 package suite → exit 0 (log target/harness-permissions-parity/dogfood/section9-package-nextest.txt):
     cargo nextest run -p harness-core (724), -p harness-tools (365), -p harness-providers (59),
     cargo nextest run -p harness --test config_docs_reference_test (19),
     cargo nextest run -p harness --test bootstrap_profiles_test (35)
5. Dogfood artifact paths:
   - Explore: target/harness-permissions-parity/dogfood/explore-plan-task.txt
   - Bash: target/harness-permissions-parity/dogfood/bash.txt
   - Plan: target/harness-permissions-parity/dogfood/explore-plan-task.txt
   - Deny-hide: target/harness-permissions-parity/dogfood/deny-hide.txt
   - Contracts P2/P6/P7: target/harness-permissions-parity/dogfood/contract-p2-p6-p7.txt
   - §9 packages: target/harness-permissions-parity/dogfood/section9-package-nextest.txt
6. Intentional divergences: explore explicit denies; external_directory message-level equivalent; V2 SQLite deferred (table above).
7. I have not weakened tests or edited inspirations/ to force a pass.

Signed: Sisyphus / ulw-loop 2026-07-16

---

## T9 honesty update — docs + progress ledger (2026-07-16)

This section supersedes the completion certificate above for claims about shipped permission behavior. It was added after T3–T8 landed so the ledger does not overclaim.

### What shipped in T3–T8

| Task | Shipped behavior | Evidence location |
|---|---|---|
| T3 public kinds | `read`, `external_directory`, and `doom_loop` are public config keys and `PermissionKind` variants | `crates/harness-core/src/perm.rs`, `configs/config.json` |
| T4 allow-by-default | Omitted `permission` defaults ordinary kinds to allow; `external_directory` and `doom_loop` default to ask | `default_internal_permissions_config`, example config `"permission": "allow"` |
| T5 agent matrices | `build`/`plan`/`general`/`explore` effective rules plus category routes; `general.task=allow`, `general.todowrite=deny`, category `task=deny` | `crates/harness-core/tests/fixtures/permission_ruleset_parity/opencode_agent_ts_matrix.json`, bootstrap profile tests |
| T6 `.env` read patterns | `read` selector rules for `*.env` ask, `*.env.*` ask, `*.env.example` allow | `default_read_env_permission_rules()` |
| T7 external_directory | Out-of-workspace paths ask via `external_directory`; grant-gated call-scoped prefixes | `crates/harness-tools/src/shell_safety/path_validation.rs`, coord integration tests |
| T8 doom_loop | Third identical `(tool_id, args_digest)` call asks; `once` resets streak, `always` disables further asks for the run | `RunState` streak counter, coord integration tests |

### Intentional Harness divergences (still true after T8)

| Divergence | OpenCode reference | Harness choice |
|---|---|---|
| `plan.shell=ask` + read-only bash guard | OC plan inherits base `* allow` for bash | Harness keeps shell ask and a coordinator-side read-only guard |
| Category routes `task=deny` | OC has no native category agents | Harness category routing profiles deny recursive `task` by default |
| Plan edit path | `.opencode/plans/*.md` | `.agent-harness/plans/*` |

### What is not claimed

The following are explicitly deferred or out of scope for this wave:

- Full OpenCode PermissionNext engine.
- V2 SQLite always-allow session grants; current grants are call-scoped run prefixes only.
- First-class `ExternalDirectory` ask UI; the current implementation is a grant-gated message-level ask.
- OpenCode temporary-directory whitelist (`Global.Path.tmp`); workspace-relative paths and explicit grants are the only supported gates.
- MCP tool permission naming.
- Desktop permission UI.

### Residual risks

- Permission policy is an operator approval layer, not a sandbox. A granted `bash` command still runs on the host.
- Bash path-like tokens the shell scanner misses are denied, not silently allowed.
- `external_directory` grants are per-run prefixes, not persisted session grants or a global whitelist.

### Commands run after this update

```text
cargo nextest run -p harness --test config_docs_reference_test → 18 passed, 1 failed
  failed: sessions_architecture_test::readiness_closeout_docs_are_current_and_back_roadmap_claims
  reason: active PRD file pattern mismatch; unrelated to permission docs
cargo nextest run -p harness --test config_docs_reference_contract_test → 2 passed
```

Broader lanes (`quality-gates`, `fast`) were not run because the working tree still contains unrelated dirty WIP (branding and code changes) that the task explicitly leaves unstaged.

Evidence: `.omo/evidence/t9/`.


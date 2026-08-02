# grok-build-clean-room-parity - Work Plan

## TL;DR (For humans)

**What you'll get:** A clean-room Harness experience that reproduces Grok Build's locally usable interface and features, backed by fresh side-by-side proof rather than inherited claims. Every visible action must perform real work and survive errors, restarts, and terminal differences.

**Why this approach:** The current tests are healthy but the existing parity signoff can validate the wrong artifact. The plan fixes proof first, preserves Harness's event-sourced safety boundaries, then divides the rewrite among isolated specialists with independent reviewers.

**What it will NOT do:** It will not copy Grok's code or test assets, revive hosted/enterprise/voice/telemetry services, hide differences behind broad branding masks, or overwrite the existing dirty worktree.

**Effort:** XL
**Risk:** High - the work spans terminal rendering, runtime owners, multiple platforms, live integrations, and strict differential evidence in a heavily modified workspace.
**Decisions to sanity-check:** All locally reproducible behavior stays in scope; Harness branding is the only allowed substitution; implementation follows clean-room role separation and test-first differential proof; no Git cleanup or publication occurs automatically.

Your next move: after the required high-accuracy plan reviews pass, start the execution plan in a separate worker session. Full execution detail follows below.

---

> TL;DR (machine): XL/high-risk clean-room parity program; 39 execution todos plus seven final gates; proof-first differential TDD, disjoint specialist writers, serial integrators, immutable dual-binary evidence, no automatic Git operations.

## Scope
### Must have
- Reimplement every locally reproducible Grok Build TUI/UX behavior and retained local feature journey against the pinned reference binary and source revision.
- Preserve Harness identity only: product name/logo/version and truthful provider/account wording may differ; geometry, colors, modifiers, content behavior, focus, cursor, timing, animation, interaction, and terminal behavior may not.
- Preserve Harness architectural invariants: events are durable truth; coordinator owns event append, task scheduling, permissions, hooks, compaction, cancellation, and lifecycle; replay is side-effect free; provider metadata is redacted before persistence.
- Cover the complete reference registries discovered during research: 74 actions, 70 action definitions, 61 slash-command modules, 64 builtin command instances, 40 settings, 11 active modals, and 20 terminal brands. Wave 0 regenerates these counts from the pinned source and fails if they drift. The frozen reference defines 74 ActionId variants but only 70 ActionDef rows: NextModel, DumpInputLog, NewSessionInWorktree, and ExitSession intentionally have no ActionDef. The count target is therefore 70, never 72 (the superseded task-spec value).
- Include local terminal/input/render/writer/cursor behavior; scrollback, transcript, selection, folding, links, clipboard, inline local media and Mermaid; prompt editing, history, paste, shell mode, file/slash completion; startup, shell lifecycle, responsive layouts, themes, overlays, pickers, settings, minimal/inline/fullscreen/vim modes, dashboard, queue/tasks/todo/subagents, permissions/questions, error/cancel/recovery.
- Include retained local owners: sessions/replay/persistence/fork/clone/rewind/crash recovery, prompt queue/interjection/compaction/memory, workspace/worktrees/trust/VCS/attribution/sandbox, tools/background tasks/scheduler/teams, hooks/local MCP/local ACP, filesystem-local plugin discovery/trust/install/reload/enable/disable/uninstall/local-path marketplace resolution and bundled agent/skill/hook/MCP activation, code graph/LSP, public auth/providers/models, updates, sleep/wake credential protection, CLI/config/doctor/support export.
- Use differential TDD: an independently authored failing contract and failure mutation precede every product slice.
- Produce fresh dual-binary evidence from the exact pinned reference and exact sealed Harness binary; bind every artifact to immutable candidate/reference/runner identities.
- Maximize safe parallelism with disjoint leaf reservations, serial shared-root integrators, independent QA owners, and an orchestration-only lead.

#### Definition: locally reproducible
- May use local filesystem/processes, deterministic mock providers, configured live providers, local MCP stdio or explicitly configured HTTP, local ACP stdio, local terminal parsers/emulators, and native display/clipboard facilities when available.
- A live/native dependency may temporarily make an in-scope proof row `blocked`; it does not remove the feature from scope, never counts as `pass`, and prevents final release until the prerequisite is supplied and the row passes.
- Every source-visible family is enumerated in the Wave 0 scope ledger. A missing implementation is `incomplete`, not an implicit exclusion.

#### Frozen reference authority
- Binary: `/home/urbanbreach/Projects/agent-harness/inspirations/grok-build/target/debug/xai-grok-pager`
- Source root: `/home/urbanbreach/Projects/agent-harness/inspirations/grok-build`
- Revision: `c1b5909ec707c069f1d21a93917af044e71da0d7`
- Binary SHA-256: `883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5`
- Version: `grok 0.1.220-alpha.4 (c1b5909) [stable]`
- Workers use these absolute paths from the shared workspace. They MUST NOT copy, symlink, rebuild, download, or relocate the reference.
- Wave 0 computes `reference_epoch = sha256(canonical_json(reference HEAD, Git tree id, clean-status proof, binary SHA/version, and every reference source file used by the inventory))`. Every reference audit/capture re-verifies this epoch before and after execution; drift invalidates the attempt.

### Must NOT have (guardrails, anti-slop, scope boundaries)
- No voice/dictation/STT/TTS actions, settings, commands, dependencies, or UI.
- No hosted/xAI-only services, SuperGrok/billing surfaces, hosted share/upload, hosted image/video generation, hosted announcements, feedback, release-note feeds, or analytics/telemetry.
- No enterprise SSO, generic browser OIDC, enterprise deployment configuration, or enterprise credential refresh.
- No remote workspace/control-plane lifecycle, remote hub client, remote bind/upload/recovery, or proxy-control surfaces.
- No remote MCP OAuth, discovery, PKCE, consent, token exchange, provisioning, or implicit endpoint expansion. Local configured transports remain in scope.
- No copied or mechanically transformed Grok source, tests, fixtures, snapshots, scenario YAML, identifiers, theme tables, captured evidence, or decompiled output.
- No inherited completion claims, task numbering, divergence approvals, copied digests, stale evidence, or historical `.omo/tui-grok-cleanroom/`, `ATTEMPT/`, old plan, or prior evidence artifacts used as current proof.
- No Git reset/clean/stash/stage/branch switch, agent-harness repository worktree creation, or archive-copy operation without a new explicit user instruction. Product-behavior worktrees may be created only inside task-owned isolated temporary fixture roots, with receipts proving the main agent-harness repository and its existing worktrees are unchanged. Existing tracked and untracked paths are user work until classified.
- No direct reference-source access by implementation workers. Reference auditors produce behavior contracts; implementers consume those contracts and fresh black-box captures only. Independent reviewers may inspect both sides.
- No TUI/CLI/tools/provider bypass of coordinator, event, permission, replay, redaction, or writer-lock authority.
- No manifest promotion before the sealed final acceptance wave. No divergence exists at plan start; any non-identity divergence requires a new exact user approval.

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: differential TDD using Rust unit/integration/nextest tests, independently authored PTY drivers, semantic-cell comparison, zero-tolerance raster comparison outside approved identity cells, ordered frame/timing traces, owner-call receipts, and explicit failure mutations.
- Evidence: `<attemptDir>/task-<N>-grok-build-clean-room-parity/` where `attemptDir` is the current ULW attempt directory; outside ULW use `.omo/evidence/grok-build-clean-room-parity/<attempt-id>/`. Every task owns only its directory and never copies artifacts forward.

### Canonical proof vector
Every in-scope behavior row declares applicable dimensions from this vector. Current L0-L6 labels may be retained only if Wave 0 maps them losslessly and bumps the schema when necessary.

| Proof | Required evidence |
|---|---|
| P0 inventory | behavior id, reference source paths/symbols, Harness owner, disposition, trigger, focus, viewport, dependencies |
| P1 contract | independently authored failing differential contract and expected reference observations |
| P2 owner | compiled public-surface owner call plus observable external postcondition |
| P3 terminal | exact input trace, PTY bytes, semantic cells, cursor, alternate-screen state, focus owner |
| P4 raster | settled reference/candidate PNGs and zero unapproved RGBA differences |
| P5 motion | ordered frames, tick timestamps, settle dwell, scroll/resize/cancel/animation timing |
| P6 rejection | stale/copy/self-oracle/wrong-binary/secret/mask-expansion/owner-bypass mutations fail closed |
| P7 lifecycle | restart, persistence, error, cancel, recovery, and teardown receipts |
| P8 external | live provider/native terminal/clipboard proof when the behavior requires it; unavailable environment is `blocked` |
| P9 review | F1-F4 independent approvals plus terminal Oracle approval; required before canonical promotion |

### Status model
- Status values: `incomplete`, `blocked`, `pass`, `diverged`.
- `blocked` is only for an external prerequisite that the task cannot supply; record the exact missing environment, command, owner, and retry condition.
- `pass` requires every applicable proof dimension from the same sealed `product_epoch`, exact Harness binary SHA, and frozen reference SHA; the compiled real owner and external postcondition must be exercised.
- `diverged` requires a new user-approved divergence id created after this plan. Historical divergence receipts are evidence-only and confer no approval.
- Only the final attestation owner may promote manifests. All implementation and integration workers leave rows `incomplete`.

### Epoch and provenance
- Wave 0 creates `scripts/parity_epoch.py`. It emits sorted canonical JSON of every product-affecting tracked/untracked source, Cargo manifest/lockfile, toolchain file, config/schema input, build script, and runtime prompt asset. It excludes `.git/`, `target/`, sessions, caches, evidence, artifacts, and `inspirations/`.
- `product_epoch = sha256(canonical_input_manifest)`. Product changes invalidate all candidate evidence.
- `reference_epoch` independently binds the frozen reference source inputs and binary. Product-epoch exclusion of `inspirations/` never exempts reference drift checks.
- Every receipt records candidate source revision/worktree digest, `product_epoch`, immutable candidate path/SHA/version/permissions, frozen reference path/SHA/version/revision/`reference_epoch`, runner path/SHA/version, command/cwd/environment, viewport/terminal capability profile, artifact SHA, secret scan, teardown, and fresh-root identity.

## Execution strategy
### Parallel execution waves
> Target 5-8 todos per wave. Fewer than 3 (except the final) means you under-split.

| Wave | Todos | Parallelism | Gate |
|---|---|---|---|
| 0 Foundation | 1-8 | Research/classification tasks may overlap; evidence/schema/shared-root changes serialize through Todo 8 | No product writer starts before truthful baseline, reservations, and proof framework pass mutations |
| 1 TUI primitives | 9-16 | Todos 9,10,11,15 may start together; 12 waits for 11, 13 waits for 9+11, 14 waits for 11; Todo 16 integrates after 9-15 | Primitive contracts and owner tests green |
| 2 Retained runtime owners | 17-24 | Todos 17-23 run on disjoint subsystem roots; Todo 24 integrates core/tool/provider/CLI roots | Real owner journeys and replay invariants green |
| 3 TUI surfaces and journeys | 25-32 | Todos 25-31 run on disjoint view/state leaves; Todo 32 integrates TUI roots | All visible local journeys reach real owners |
| 4 Differential hardening and seal | 33-39 | Todo 33 authors undisclosed holdouts; 34-37 run after 33; Todo 38 integrates evidence; Todo 39 seals candidate | Fresh complete proof set on one candidate revision |
| Final verification | F1-F7 | F1-F4 parallel; F5 proposes; F6 approves; F7 applies mechanically | Unconditional approval precedes canonical status promotion |

### Agent and reservation contract
- Lead agent: schedules dependencies, acquires/releases path and global locks, validates receipts, routes blockers, and runs integration verification. It MUST NOT write product code, capture reference evidence, mutate manifests, or silently repair worker patches.
- Coding workers: category `deep` or `ultrabrain`; load `karpathy-guidelines`, `programming`, `rust-best-practices`, plus `rust-async-patterns` for async/runtime work.
- Visual/TUI workers: category `visual-engineering`; load `karpathy-guidelines`, `programming`, `rust-best-practices`, `frontend`, and `visual-qa`.
- Reference auditors: `explore`, read-only, no product/test writes. They may inspect the frozen source and run the frozen binary.
- QA/review workers are independent of implementers and receive undisclosed mutations/holdouts.
- All workers use the shared workspace. No worktrees or branches may be created in the agent-harness repository; task-owned fixture repositories may create isolated worktrees only under the exception in the Must NOT Have guardrail. Disjoint exclusive write sets are the concurrency boundary.
- A worker refuses to start when any reserved path differs from its recorded preimage hash or when another active reservation overlaps it.
- Final `task(subagent_type="oracle")` calls are host OpenCode orchestration tools supplied to the plan executor, not profiles resolved through the Harness product's `.agent-harness/agents` catalog. Before F1, the lead performs a host-tool availability preflight; absence of the host Oracle route blocks release rather than substituting a product profile.

### Per-slice differential RED gate
- Todo 8 publishes sealed reference observations and contract templates for every implementation slice.
- Before any product worker in Todos 9-31 starts, a separately routed QA owner writes the behavior-named failing test/driver in that todo's reserved test path, runs it against the current Harness candidate, records the expected failure plus a validated failure mutation, and signs `<attemptDir>/task-<N>-grok-build-clean-room-parity/red-receipt.json` bound to `reference_epoch`.
- The scheduler in Todo 7 refuses implementation dispatch without a valid RED receipt. The implementation worker cannot edit the reference observation, expected assertions, failure mutation, or QA receipt.
- Todo 33 adds only undisclosed final holdouts; it is not the first author of implementation contracts.

### Serial shared roots
Only the named wave integrator may edit: workspace/crate `Cargo.toml`, `Cargo.lock`, every crate root `src/lib.rs`, top-level CLI/tool/provider registries, `crates/harness-tui/src/app.rs`, `crates/harness-tui/src/ui.rs`, `crates/harness-tui/src/keybindings.rs`, `crates/harness-tui/src/overlay.rs`, public config/schema files, public manifests, `scripts/test-lanes.sh`, and `scripts/harness-qa-dogfood.sh`.

### Global resource locks
- `REFERENCE_CAPTURE`: frozen reference process/capture.
- `CANDIDATE_BINARY`: installed sealed Harness binary and candidate identity.
- `PTY_NATIVE_DISPLAY`: PTY, nested terminal, clipboard, X11/Wayland/native screenshot resources.
- `LIVE_PROVIDER`: credentials, provider budget, and live configuration.
- `CONFIG_INTEGRATION_REGISTRY`: integration refresh and MCP connection/tool registry invalidation.
- `MANIFEST_PROMOTION`: capability/TUI manifest writes.
- `EVIDENCE_EPOCH`: final evidence root and attestation inputs.
- `FULL_GATE_RUNNER`: workspace-wide format/clippy/nextest/coverage/signoff execution.

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 | - | 2-8 | - |
| 2 | 1 | 3-8, 9-39 | 3 |
| 3 | 1 | 8, 17-32 | 2 |
| 4 | 1,2 | 5-8, 9-39 | 3 |
| 5 | 4 | 8, 9-39 | 6,7 |
| 6 | 2,4 | 8, 9-39 | 5,7 |
| 7 | 1,3,4 | 8, 9-39 | 5,6 |
| 8 | 2-7 | 9-39 | - |
| 9,10,11,15 | 8 | 12-16 as named | each other when exact reservations are disjoint |
| 12 | 8,11 | 16,27,29,33-35 | 13,14,15 after their own prerequisites |
| 13 | 8,9,11 | 16,25,28,33-35 | 12,14,15 after their own prerequisites |
| 14 | 8,11 | 16,25-32,34 | 12,13,15 after their own prerequisites |
| 16 | 9-15 | 17-32 | - |
| 17-19 | 8,16 | 24, 25-31 as named | 20-23 after exact reservation graph validation |
| 20 | 8,16 | 24,29-32 | 17-19,22,23; excludes MCP/code-LSP/integration/config paths |
| 21 | 8,16 | 24,29-32 | 17-20,22; owns MCP/code-LSP/integration leaves but no config paths |
| 22 | 8,16 | 24,28-32,37 | 17-21; owns auth/provider runtime leaves but no config/catalog paths |
| 23 | 8,16 | 24,25-32 | 17-22 after exact reservations; sole owner of config/catalog leaves |
| 24 | 17-23 | 25-32 | - |
| 25-31 | 16, named runtime owners, 24 where required | 32 | each other when write sets remain disjoint |
| 32 | 24-31 | 33-39 | - |
| 33 | 32 | 34-38 | - |
| 34-37 | 32,33 | 38 | each other under distinct evidence/global locks |
| 38 | 33-37 | 39 | - |
| 39 | 38 | F1-F4 | - |
| F1-F4 | 39 | F5 | each other |
| F5 | F1-F4 | F6 | - |
| F6 | F5 | F7 | - |
| F7 | F6 | completion | - |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. Freeze the truthful starting state and reference authority
  What to do / Must NOT do: A read-only `explore` auditor records branch, HEAD, porcelain-v2 status, staged/unstaged/untracked path lists, preimage SHA-256/mode/type for every dirty path, current manifest counts, Cargo/toolchain identity, existing worktrees, and the frozen reference path/revision/version/SHA/clean status. It also emits a canonical reference-source input manifest (Git tree id plus every source file later cited by the inventory), computes `reference_epoch`, and writes the evidence-local validator `verify_starting_state.py`. Permitted outputs are only `<attemptDir>/task-1-grok-build-clean-room-parity/{starting-state.json,reference-inputs.json,verify_starting_state.py,commands/**}`. MUST NOT edit product/source files, copy user work, stage, stash, reset, clean, branch-switch, create repository worktrees, or rebuild the reference.
  Parallelization: Wave 0 | Blocked by: none | Blocks: 2-8
  References: `AGENTS.md`; `.gitignore`; `docs/tui-reference-parity-manifest.v1.json`; `docs/capability-inventory.v1.json`; `.omo/ulw-research/20260727-grok-build-clean-room-parity-plan/SYNTHESIS.md`; frozen reference authority in this plan.
  Acceptance criteria (agent-executable): `git status --porcelain=v2 --branch`, `git diff --stat`, `git diff --cached --stat`, `git worktree list --porcelain`, `sha256sum "$REFERENCE_BIN"`, `"$REFERENCE_BIN" --version`, `git -C "$REFERENCE_ROOT" rev-parse HEAD`, `git -C "$REFERENCE_ROOT" rev-parse HEAD^{tree}`, and `git -C "$REFERENCE_ROOT" status --porcelain` are captured; `python3 <attemptDir>/task-1-grok-build-clean-room-parity/verify_starting_state.py --state <attemptDir>/task-1-grok-build-clean-room-parity/starting-state.json --reference-inputs <attemptDir>/task-1-grok-build-clean-room-parity/reference-inputs.json` exits 0 and prints `starting_state_valid=true reference_epoch=<sha256>`.
  QA scenarios: Happy: rerun the inventory verifier and expect `starting_state_valid=true`. Failure: mutate one copied inventory entry in a temporary receipt and expect digest/path-mode rejection without touching the workspace. Evidence `<attemptDir>/task-1-grok-build-clean-room-parity/`.
  Commit: N | Read-only evidence task.

- [ ] 2. Generate the exhaustive frozen-reference behavior inventory
  What to do / Must NOT do: A read-only reference-auditor swarm enumerates every action/action definition, slash command/alias/argument, setting, modal/picker/overlay, focus transition, terminal conditional, view, runtime feature owner, documented journey, PTY/scenario family, and observable fallback from the pinned source/binary. Emit `reference-inventory.json`, independent black-box probe specifications, and the evidence-local `validate_reference_inventory.py` under this task's evidence root. Every row includes source path/symbol, trigger, focus owner, state transition, rendered effect, side effect, persistence, viewport/capability conditions, approved disposition, and P0-P9 applicability. MUST NOT copy source/test/fixture/YAML contents or assign pass status.
  Parallelization: Wave 0 | Blocked by: 1 | Blocks: 4,6,8,9-39 | Can run with: 3
  References: `inspirations/grok-build/crates/codegen/xai-grok-pager/src/actions/mod.rs`; `inspirations/grok-build/crates/codegen/xai-grok-pager/src/actions/defaults.rs`; `inspirations/grok-build/crates/codegen/xai-grok-pager/src/slash/commands/mod.rs`; `inspirations/grok-build/crates/codegen/xai-grok-pager/src/settings/defs.rs`; `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/modal.rs`; `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/app_view.rs`; `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/`; `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/terminal/mod.rs`; `.omo/ulw-research/20260727-grok-build-clean-room-parity-plan/wave-2-explore-interaction-inventory.md`; `.omo/ulw-research/20260727-grok-build-clean-room-parity-plan/wave-2-explore-reference-proof.md`.
  Acceptance criteria (agent-executable): `python3 <attemptDir>/task-2-grok-build-clean-room-parity/validate_reference_inventory.py --inventory <attemptDir>/task-2-grok-build-clean-room-parity/reference-inventory.json --source-root "$REFERENCE_ROOT" --reference-inputs <attemptDir>/task-1-grok-build-clean-room-parity/reference-inputs.json --expect actions=74 --expect action_defs=70 --expect slash_modules=61 --expect builtin_commands=64 --expect settings=40 --expect active_modals=11 --expect terminal_brands=20` exits 0 and prints `inventory_valid=true reference_epoch=<same-sha>`; duplicate/missing ids, absent source symbols, grouped catch-all rows, and exclusion leakage fail.
  QA scenarios: Happy: run the exact validator command above before and after the audit and require unchanged `reference_epoch`. Failure: delete one action, add one duplicate behavior id, or dirty one cited reference source in a temporary copy of the input manifest; validator reports every defect. Evidence `<attemptDir>/task-2-grok-build-clean-room-parity/`.
  Commit: N | Reference audit output is integrated only by Todo 8.

- [ ] 3. Classify every current dirty path and regenerate the scope taxonomy
  What to do / Must NOT do: A read-only `explore` auditor assigns every modified/untracked path from Todo 1 exactly one classification: `retain-and-prove`, `rework`, `replace`, `retire-approved`, `unrelated-preserve`, `evidence-only`, or `suspected-contamination`. It separately classifies every reference feature family/sub-capability as `implement`, `retain-and-prove`, `identity-substitute`, `approved-exclusion`, or `external-proof-blocked`, and writes the evidence-local `validate_classification.py`. Task-numbered tests are `rework`, never trusted because of their names. Direct reference paths are allowed only in planning/reference-audit/capture tooling, never product/runtime fixtures. MUST NOT rename, delete, restore, copy, or edit any classified path.
  Parallelization: Wave 0 | Blocked by: 1 | Blocks: 7,8,17-32 | Can run with: 2
  References: `docs/scope-removal-ledger.v1.json`; `wave-2-explore-scope-taxonomy.md`; root/crate `AGENTS.md`; Todo 1 starting-state inventory.
  Acceptance criteria (agent-executable): `python3 <attemptDir>/task-3-grok-build-clean-room-parity/validate_classification.py --starting-state <attemptDir>/task-1-grok-build-clean-room-parity/starting-state.json --classification <attemptDir>/task-3-grok-build-clean-room-parity/path-classification.json --scope <attemptDir>/task-3-grok-build-clean-room-parity/scope-taxonomy.json --approved-scope .omo/drafts/grok-build-clean-room-parity.md` exits 0 and prints `coverage=100% overlaps=0 unapproved_exclusions=0 pass_claims=0`.
  QA scenarios: Happy: run the exact validator command above. Failure: insert an unclassified path, dual classification, unapproved exclusion, or `pass` claim; validator returns nonzero and names all defects. Evidence `<attemptDir>/task-3-grok-build-clean-room-parity/`.
  Commit: N | Read-only classification output is integrated only by Todo 8.

- [ ] 4. Implement immutable evidence, receipt, and product-epoch contracts
  What to do / Must NOT do: A `deep` Rust worker extends `crates/harness-testkit/src/parity/{provenance,status,artifact_schema,artifact_schema/**}` and adds `scripts/parity_epoch.py` so receipts bind P0-P9 evidence to fresh roots, exact candidate/reference/runner identities, canonical product input manifests, `reference_epoch`, secret scans, teardown, and applicable proof dimensions. Preserve existing valid consumers through a schema migration, not permissive compatibility. MUST NOT make timestamps sufficient freshness proof or allow evidence from differing product/reference epochs.
  Parallelization: Wave 0 | Blocked by: 1,2 | Blocks: 5-8,9-39 | Exclusive writes: named parity provenance/status/schema modules and `scripts/parity_epoch.py`
  References: `crates/harness-testkit/AGENTS.md`; `crates/harness-testkit/src/parity/provenance.rs`; `status.rs`; `artifact_schema.rs`; `artifact_schema/validation.rs`; `crates/harness-testkit/src/secret_scanner.rs`; `wave-2-explore-reference-proof.md`.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-testkit --test parity_artifact_schema_test --test parity_cells_test`; `python3 scripts/parity_epoch.py --self-test`; two identical input sets yield one epoch, one source-byte mutation changes it, and excluded runtime/evidence/reference files do not.
  QA scenarios: Happy: validate a fresh cross-source receipt. Failure: wrong candidate SHA, mismatched epoch, self-oracle, copied artifact, stale root, secret, missing teardown, unknown proof dimension, and artifact outside task root are all rejected. Evidence `<attemptDir>/task-4-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless the execution user explicitly requests commits.

- [ ] 5. Repair exact-binary parity runners and acceptance-lane ownership
  What to do / Must NOT do: A `deep` worker repairs `scripts/tui-parity/**`, PTY/capture support, and parity lane tests so reference captures execute the frozen absolute reference and candidate captures execute the explicit absolute `HARNESS_BIN`. Remove helper-binary substitution, copied reference-digest seeding, silent skips, and accepting dry runs. Keep `scripts/test-lanes.sh` and `scripts/harness-qa-dogfood.sh` changes reserved for Todo 8; provide failing tests and a leaf patch contract for the integrator. MUST NOT use `cargo run` or current-process self-rendering for signoff.
  Parallelization: Wave 0 | Blocked by: 4 | Blocks: 8,9-39 | Can run with: 6,7
  References: `scripts/tui-parity/web-terminal-visual-qa.mjs`; `capture-resp-idle-shell-l3.sh`; `crates/harness-tui/tests/support/reference_parity_pty_impl.rs`; `reference_parity_provenance.rs`; O-008/O-009 in the research journal.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test reference_parity_runner_identity_test --test reference_parity_evidence_test`; `node scripts/tui-parity/web-terminal-visual-qa.mjs --self-test --chrome-bin /home/urbanbreach/.cache/ms-playwright/chromium-1228/chrome-linux64/chrome`; absolute binary path/SHA/version/permissions are recorded and used; `HARNESS_BIN` omission/mismatch fails; reference and candidate process ids/paths differ; no copied digest satisfies freshness.
  QA scenarios: Happy: run one startup capture against both exact binaries with a fresh evidence root. Failure: point `HARNESS_BIN` at the helper binary and expect runner-identity rejection; replay a prior artifact and expect freshness/copy rejection. Evidence `<attemptDir>/task-5-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 6. Restrict identity substitution and strengthen semantic/raster comparators
  What to do / Must NOT do: A `deep` worker updates `crates/harness-testkit/src/parity/{cells,compare,frame_io,vt100_adapter}` and identity comparator tests. Identity fields are semantic named regions limited to Harness logo, product title, version, and truthful provider/account text; masks validate bounds, non-overlap, stable geometry, and grapheme-only substitution. Colors, modifiers, cursor, focus, spacing, borders, transcript/footer content, animation, and timing remain unmasked. MUST NOT inherit historical mask coordinates without fresh source-backed contracts.
  Parallelization: Wave 0 | Blocked by: 2,4 | Blocks: 8,9-39 | Can run with: 5,7
  References: `crates/harness-testkit/src/parity/compare.rs`; `cells.rs`; `crates/harness-tui/tests/reference_parity_identity_comparator_test.rs`; O-010/O-011; user identity decision.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-testkit --test parity_cells_test` and `cargo nextest run -p harness-tui --test reference_parity_identity_comparator_test`; only approved identity grapheme substitutions pass when all other cells/properties are exact.
  QA scenarios: Happy: substitute Harness identity text inside declared semantic bounds. Failure: expand a mask into transcript/footer, move the cursor, alter border/spacing/color/modifier, overlap masks, or mask a spinner frame; each is rejected. Evidence `<attemptDir>/task-6-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 7. Enforce scheduler reservations, clean-room roles, and task receipts
  What to do / Must NOT do: An `ultrabrain` worker updates `scripts/parity_task_qa.py` with `validate-reference-observations` and `validate-scenarios` subcommands, adds `scripts/validate_grok_reference_inventory.py`, `scripts/validate_dirty_classification.py`, and `scripts/run-parity-review.py`, and enforces exact-file reservations generated from Todo 3. Every implementation task declares preimage hashes, exclusive files, shared-root owner, dependencies, skills, independent RED-gate QA owner, global locks, expected postcondition, failure mutation, and evidence root. The scheduler rejects implementation dispatch without a valid RED receipt bound to Todo 8 observations. Add a clean-room scanner that permits frozen-reference paths only in approved auditor/planning/capture files and detects suspicious copied identifiers/large structural matches for independent review. MUST NOT auto-revert or auto-copy user files.
  Parallelization: Wave 0 | Blocked by: 1,3,4 | Blocks: 8,9-39 | Can run with: 5,6
  References: `scripts/parity_task_qa.py`; `crates/harness-testkit/tests/parity_scheduler_test.rs`; `AGENTS.md` task semantics; `wave-2-explore-topology-redteam.md`.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-testkit --test parity_scheduler_test`; `python3 scripts/parity_task_qa.py --self-test`; `python3 scripts/parity_task_qa.py validate-scenarios --self-test --require-happy 1 --require-failure 1 --require-mutation 1 --require-coverage 100`; `python3 scripts/validate_grok_reference_inventory.py --self-test`; `python3 scripts/validate_dirty_classification.py --self-test`; `python3 scripts/run-parity-review.py --self-test`; the exact Todo 33 invocation is exercised by the self-test; reservation overlap, changed preimage, wildcard overlap, missing RED receipt/QA owner, missing scenario family, copied scenario asset, out-of-write-set edit, duplicate patch, copied reference path, and lead-agent product/manifest edit are rejected.
  QA scenarios: Happy: two disjoint exact-file leaf tasks with valid RED receipts schedule concurrently, a shared-root integrator waits, and the Todo 33 scenario validator reports 100% coverage. Failure: overlapping wildcard expansion, unreserved `lib.rs`, wrong preimage, missing RED receipt, missing happy/failure/mutation scenario, copied reference asset, or implementation worker reference-source access fails before dispatch. Evidence `<attemptDir>/task-7-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 8. Integrate the truthful foundation and publish canonical incomplete manifests
  What to do / Must NOT do: The sole Wave 0 `deep` integrator verifies Todos 1-7 file-by-file, updates required crate roots/manifests, integrates `scripts/test-lanes.sh` and `scripts/harness-qa-dogfood.sh`, publishes current-source-generated canonical inventories/manifests, and dispatches a read-only reference-capture swarm that executes the exact frozen binary for every implementation slice. Seal normalized reference observations, expected text/cells/frames/timing/state/side-effect contracts, and failure mutations under `<attemptDir>/task-8-grok-build-clean-room-parity/reference-observations/<slice>/`, each bound to `reference_epoch`. All in-scope rows remain `incomplete`; exclusions live in scope/removal ledgers. Rename inherited task-numbered test modules only after confirming no external path dependency. MUST NOT promote status, delete unclassified work, import old task structure, or let implementation workers read raw reference source.
  Parallelization: Wave 0 serial integration gate | Blocked by: 2-7 | Blocks: 9-39 | Exclusive writes: all shared roots listed in Execution strategy
  References: outputs/receipts from Todos 1-7; `docs/AGENTS.md`; `configs/AGENTS.md`; `scripts/AGENTS.md`; every crate `AGENTS.md` affected by root integration.
  Acceptance criteria (agent-executable): `cargo fmt --all -- --check`; `cargo check --workspace`; `cargo nextest run -p harness-testkit --test parity_artifact_schema_test --test parity_scheduler_test --test parity_cells_test`; `cargo nextest run -p harness-tui --test reference_parity_manifest_test --test reference_parity_evidence_test --test reference_parity_identity_comparator_test --test reference_parity_runner_identity_test`; `python3 scripts/validate_grok_reference_inventory.py --inventory docs/grok-reference-interaction-inventory.v1.json --source-root "$REFERENCE_ROOT" --reference-bin "$REFERENCE_BIN" --reference-inputs <attemptDir>/task-1-grok-build-clean-room-parity/reference-inputs.json`; `python3 scripts/parity_task_qa.py validate-reference-observations --inventory docs/grok-reference-interaction-inventory.v1.json --root <attemptDir>/task-8-grok-build-clean-room-parity/reference-observations --reference-epoch <reference-epoch>`; every Todo 9-31 slice has sealed observations/contract/mutation and manifests contain zero `pass`/`diverged`.
  QA scenarios: Happy: exact reference captures and scheduler self-tests pass; a sample independent QA owner creates a RED test from a sealed observation and scheduler accepts the receipt. Failure: dirty reference source, helper binary, missing slice observation, stale artifact, copied digest, mask expansion, overlapping reservation, absent RED receipt, or one false `pass` is rejected. Evidence `<attemptDir>/task-8-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 9. Reimplement terminal input decoding and capability fallbacks
  What to do / Must NOT do: A `visual-engineering` Rust worker implements the Grok-observable key, focus, paste, mouse, clipboard, hyperlink-modifier, Unicode-width, and reduced-terminal input contracts in dedicated terminal/input leaves. Consume focus-in/focus-out CSI sequences instead of inserting `[I`/`[O`; preserve terminal-specific fallbacks for VS Code-family, Apple Terminal, Windows/WSL, SSH, multiplexers, and unreliable modifiers. Use the generated behavior contract and black-box captures; MUST NOT read raw reference source or edit keybinding/root dispatch files reserved to Todo 16.
  Parallelization: Wave 1 | Blocked by: 8 | Blocks: 16,25-32 | Exclusive writes: `crates/harness-tui/src/terminal.rs`, `terminal/**`, `mouse.rs`, `clipboard_leaf.rs`, new input leaves, behavior-named tests
  References: `crates/harness-tui/AGENTS.md`; `crates/harness-tui/src/runtime.rs:TerminalCapabilityState`; `wave-2-explore-interaction-inventory.md`; Todo 8 inventory rows for terminal/input.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test terminal_input_parity_test --test responsive_terminal_theme_mouse_clipboard_leaf_test`; exact reference/candidate PTY traces agree for focus, bracketed paste, multiline, mouse wheel/click, selection, clipboard, hyperlink activation, and terminal fallback profiles.
  QA scenarios: Happy: send focus-in, type Unicode text, paste multiline content, select/copy, and open a link under supported capability profiles. Failure: malformed/incomplete CSI and unsupported modifier paths remain recoverable and never leak bytes into the composer. Evidence `<attemptDir>/task-9-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 10. Reimplement terminal lifecycle, synchronized writer, cursor, and clocks
  What to do / Must NOT do: A `visual-engineering` worker implements observable startup/teardown, alternate-screen/inline/minimal transitions, synchronized frame markers, cursor visibility/style/position deduplication, idle zero-write behavior, bounded live-update drain, resize debounce, animation demand, scroll clock, and clean child-process suspend/resume. Keep OS/process effects behind runtime leaves and guards. MUST NOT change durable app/session state or top-level render dispatch.
  Parallelization: Wave 1 | Blocked by: 8 | Blocks: 16,21,33-35 | Exclusive writes: `crates/harness-tui/src/runtime.rs`, new writer/cursor/clock leaves, runtime tests
  References: `crates/harness-tui/src/runtime.rs`; `inspirations/.../xai-grok-pager-render/src/render/draw.rs` only through Todo 8 behavior contract; `wave-1-explore-grok-tui-architecture.md`; terminal environment inventory.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test terminal_runtime_parity_test`; frame traces prove synchronized begin/end pairing, no redundant idle cell output, stable cursor commands, 16ms-class scroll flushing where required, resize debounce, and complete teardown after happy/error/cancel paths.
  QA scenarios: Happy: launch, stream, resize burst, suspend for child, resume, and exit with terminal restored. Failure: panic/error/cancel during a frame still closes synchronization and restores raw/alternate/cursor state. Evidence `<attemptDir>/task-10-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 11. Build deterministic render surfaces and frame observation seams
  What to do / Must NOT do: A `visual-engineering` worker makes Harness rendering observable as semantic frames and native/xterm raster frames without changing product behavior. Provide stable cell extraction, frame sequence ids, cursor/alternate-screen metadata, hyperlink/media placement metadata, and test-only render drivers that exercise the real painters. MUST NOT create fixture-only alternate painters or self-oracle captures.
  Parallelization: Wave 1 | Blocked by: 8 | Blocks: 16,25-39 | Exclusive writes: `crates/harness-tui/src/render_test.rs`, new render-observation leaves, behavior-named render tests
  References: `crates/harness-tui/src/render_test.rs`; `crates/harness-testkit/src/parity/{cells,frame_io,vt100_adapter}.rs`; Todo 6 comparator contract; `crates/harness-testkit/AGENTS.md`.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test deterministic_render_test --test render_observation_parity_test`; a frame captured through the real binary and deterministic backend normalizes to identical semantic metadata for the same scenario, while source identity remains distinct.
  QA scenarios: Happy: capture startup, draft, stream, overlay, scroll, and responsive frames. Failure: fixture-only renderer, dimension mismatch, missing cursor/alternate metadata, or self-comparison is rejected. Evidence `<attemptDir>/task-11-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 12. Reimplement scrollback state, layout, folding, selection, and follow behavior
  What to do / Must NOT do: A `visual-engineering` worker creates/updates ephemeral transcript view-state and scrollback leaves for entry indexing, cached heights, dirty-height invalidation, running/finish-flash state, follow mode, long-session offsets, turns, sticky prompts, fold modes, raw mode, selection geometry, scrollbars, and Unicode-safe wrapping. Durable message/tool truth remains event-derived in `SessionProjection`; view-only fold/selection state MUST NOT be persisted as events.
  Parallelization: Wave 1 | Blocked by: 8,11 | Blocks: 16,27,29,33-35 | Exclusive writes: transcript/scrollback state leaves and `ui_transcript_layout.rs`, `ui_transcript_selection.rs`, `ui_transcript_scrollbar.rs`; no top-level `ui_transcript.rs`
  References: `crates/harness-tui/src/app/session_projection.rs`; `ui_transcript_layout.rs`; `ui_transcript_selection.rs`; `ui_transcript_scrollbar.rs`; `wave-2-explore-architecture-crosswalk.md`.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test scrollback_state_parity_test --test reference_parity_tx_transcript_test`; matched reference/candidate traces for follow/unfollow, page/half-page/top/bottom/turn navigation, fold/expand/raw, sticky prompts, selection/copy, and >u16-height transcripts.
  QA scenarios: Happy: stream a long Unicode/tool transcript, scroll away and back, fold blocks, select across wraps, then resume follow. Failure: resize invalidates cached heights without panic or selection corruption; malformed wide graphemes never split cells. Evidence `<attemptDir>/task-12-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 13. Reimplement prompt editor, history, paste, shell mode, and completions
  What to do / Must NOT do: A `visual-engineering` worker reworks composer/prompt leaves for cursor/selection, undo/redo, history search, multiline policy, bracketed paste, shell `!` mode, slash completion, file mentions, suggestion control, argument pickers, empty-enter/send-now semantics, and type-to-dismiss startup transfer. Use independent behavior rows; MUST NOT edit global keybinding registration or top-level app dispatch.
  Parallelization: Wave 1 | Blocked by: 8,9,11 | Blocks: 16,25,28,33-35 | Exclusive writes: `crates/harness-tui/src/app/composer.rs`, `slash.rs`, `slash/**`, file-mention/completion/prompt leaves, prompt tests
  References: `crates/harness-tui/src/app/composer.rs`; `crates/harness-tui/src/slash.rs`; Todo 8 reference interaction inventory; `wave-2-explore-interaction-inventory.md`.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test prompt_editor_completion_parity_test --test slash_leaf_contract_test`; reference/candidate state and render agreement for editing, selection, history, paste, slash/file filtering, argument step-back, shell mode, multiline, and submit/interject distinctions.
  QA scenarios: Happy: type, undo/redo, paste Unicode multiline, select a file and slash command, enter/exit shell mode, submit and restore history. Failure: unsupported command, missing required args, invalid completion, and disabled replay composer are recoverable and non-mutating. Evidence `<attemptDir>/task-13-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 14. Reimplement layout, chrome, themes, and responsive geometry
  What to do / Must NOT do: A `visual-engineering` worker rebuilds frame planning, shell chrome, composer geometry, breadcrumbs/warnings, footer grammar, model badge placement, theme tokens, truecolor/basic/no-color adaptation, and the required viewport matrix. Remove persistent sidebar/control-dock topology from the primary shell. MUST NOT add alternate competing shells or mask geometry/color differences.
  Parallelization: Wave 1 | Blocked by: 8,11 | Blocks: 16,25-32,34 | Exclusive writes: `crates/harness-tui/src/layout.rs`, `theme.rs`, `theme_leaf.rs`, `responsive.rs`, `ui_chrome.rs`, chrome/layout/theme tests
  References: `crates/harness-tui/DESIGN.md`; `layout.rs`; `theme.rs`; `ui_chrome.rs`; `responsive.rs`; Todo 8 visual contracts; fresh captures O-007/O-011.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test shell_topology_contract_test --test reference_parity_responsive_test --test theme_terminal_capability_parity_test`; semantic/raster comparison covers 60x20, 79x24, 80x24, 100x30, 120x32, 120x40, 120x50, and >120-wide.
  QA scenarios: Happy: startup and live shells preserve exact reference anatomy across viewport/color profiles with Harness identity substitutions only. Failure: one-cell border, footer, model badge, breakpoint, background, or sidebar mutation fails. Evidence `<attemptDir>/task-14-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 15. Implement action/effect, focus, and overlay-controller vertical seams
  What to do / Must NOT do: An `ultrabrain` worker adds leaf controllers that map the complete generated action registry to focus-aware UI intents, async runtime/coordinator effects, modal/overlay stack transitions, confirmation policy, and dashboard contexts. One action yields one explicit intent/effect decision; owner work stays outside TUI. MUST NOT edit `app.rs`, `keybindings.rs`, `overlay.rs`, `ui.rs`, or fabricate success for unowned actions.
  Parallelization: Wave 1 | Blocked by: 8 | Blocks: 16,25-32 | Exclusive writes: new `app/action_dispatch/**`, `app/focus/**`, `app/overlay_controller/**`, leaf action modules and tests
  References: `crates/harness-tui/src/app/lifecycle.rs:UiIntent`; `leaf_actions.rs`; `overlay.rs`; generated action inventory; `wave-2-explore-architecture-crosswalk.md`.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test action_effect_focus_parity_test --test tui_leaf_contract_test`; all in-scope actions have exactly one context-aware route and all excluded actions are absent/rejected.
  QA scenarios: Happy: prompt, scrollback, modal, dashboard, and overlay contexts route the same trigger differently where the contract requires. Failure: missing owner, duplicate route, wrong focus, overlay underflow, excluded command, or mutating replay intent fails. Evidence `<attemptDir>/task-15-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 16. Integrate the TUI primitive foundation
  What to do / Must NOT do: The sole Wave 1 `visual-engineering` integrator reviews Todos 9-15, wires modules through `crates/harness-tui/src/{lib,app,ui,keybindings,overlay}.rs` and `Cargo.toml`, updates `DESIGN.md` to the measured contract, and resolves only integration defects. Maintain one primary shell and event-derived durable state. MUST NOT broaden scope, add product features, or rewrite leaf logic without returning a typed finding to its owner.
  Parallelization: Wave 1 serial integration gate | Blocked by: 9-15 | Blocks: 17-32 | Exclusive writes: TUI shared roots and crate manifest
  References: Todos 9-15 receipts; `crates/harness-tui/AGENTS.md`; `crates/harness-tui/src/app/AGENTS.md`; `DESIGN.md`.
  Acceptance criteria (agent-executable): `cargo fmt --all -- --check`; `cargo check -p harness-tui`; `cargo nextest run -p harness-tui --test deterministic_render_test --test terminal_input_parity_test --test terminal_runtime_parity_test --test scrollback_state_parity_test --test prompt_editor_completion_parity_test --test shell_topology_contract_test --test action_effect_focus_parity_test`; `lsp_diagnostics` clean on changed Rust files.
  QA scenarios: Happy: launch the real Harness binary in a PTY, exercise startup→draft→idle→stream→scroll→overlay→exit across 80x24 and 120x32. Failure: focus CSI, resize burst, renderer error, and teardown interruption remain recoverable. Run `bash scripts/harness-qa-dogfood.sh --slug primitive-foundation` with a fresh evidence root. Evidence `<attemptDir>/task-16-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 17. Prove and complete sessions, persistence, replay, lineage, rewind, and recovery
  What to do / Must NOT do: A `deep` worker uses differential TDD to retain or rework local session creation/list/resume/rename, append-only storage/writer locks, replay projections, tree/fork/clone cutoffs, prompt rewind with atomic workspace restore, crash scan/reopen, foreign-session import, and support-ready metadata. Add owner receipts for filesystem changes and replay purity. MUST NOT change event history in place, execute effects during replay, accept active/writer-locked sources, or edit CLI/root registries reserved to Todo 24.
  Parallelization: Wave 2 | Blocked by: 8,16 | Blocks: 24,26,27-32 | Exclusive writes: `harness-core` session/store/lineage/rewind/crash/foreign-import leaves and behavior-named tests
  References: `crates/harness-core/AGENTS.md`; `crates/harness-core/src/{store,session_lineage,prompt_rewind,crash_recovery}.rs`; `crates/harness/src/sessions/**`; `docs/sessions-and-replay.md`; Todo 8 session inventory.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-core --test sessions_persistence_rewind_parity_test`; `cargo nextest run -p harness --test replay_sessions_cli_test`; two replay passes produce identical projections and zero provider/tool/hook/MCP/network calls.
  QA scenarios: Happy: create, rename, fork at stable cutoff, clone latest stable prefix, rewind workspace, crash/reopen, import, replay, and export metadata. Failure: noncontiguous seq, active writer lock, corrupt/truncated log, invalid cutoff, path escape, and replay effect attempt fail without mutation. Evidence `<attemptDir>/task-17-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 18. Prove and complete prompt queue, interjection, compaction, and local memory
  What to do / Must NOT do: An `ultrabrain` async worker implements reference-equivalent queue enqueue/list/dequeue, automatic post-turn drain, double-enter send-now, mid-turn interjection/reconciliation, compaction triggers/checkpoints/summaries, local durable memory scopes/search/flush, and TUI-ready event projections. Preserve coordinator authority and immutable event history. Local vector/embedding behavior remains in scope: use a deterministic local provider seam for tests and explicit configured provider for live proof. MUST NOT silently reduce memory to key-value behavior or rewrite `events.jsonl` during compaction.
  Parallelization: Wave 2 | Blocked by: 8,16 | Blocks: 24,28,29-32 | Exclusive writes: `prompt_queue*`, `memory/**`, `coord/compaction/**`, queue/interjection/compaction/memory tests
  References: `crates/harness-core/src/prompt_queue.rs`; `memory.rs`; `coord/compaction.rs`; root invariants; Todo 8 feature inventory.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-core --test prompt_queue_compaction_memory_parity_test`; queue/interjection state survives restart, compaction writes checkpoint artifacts/events without rewriting source events, and deterministic memory search ordering is stable.
  QA scenarios: Happy: enqueue during a turn, interject, send-now, auto-drain, compact near threshold, store/search workspace and global memories, restart and resume. Failure: duplicate queue id, cancelled turn, failed compaction model call, corrupt checkpoint, embedding/provider unavailability, and redaction violation remain recoverable with truthful events. Evidence `<attemptDir>/task-18-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 19. Prove and complete local workspace, worktrees, trust, VCS attribution, and sandbox
  What to do / Must NOT do: An `ultrabrain` worker implements local workspace root/path safety, folder trust, create/list/select/cleanup worktrees, per-session isolation, Git/Jujutsu status, edit attribution/blame/diff/revert, checkpoints, and Linux/macOS/Windows sandbox profiles including network confinement where the platform supports it. Retain local behavior while removing every remote-hub path. MUST NOT relax path safety, follow unsafe symlinks, share event stores across worktrees, or claim unsupported platform enforcement.
  Parallelization: Wave 2 | Blocked by: 8,16 | Blocks: 24,25,30-32 | Exclusive writes: workspace/worktree/trust/VCS/attribution/sandbox leaves and tests
  References: `crates/harness-core/src/{workspace,worktree,folder_trust,jujutsu,edit_attribution}.rs`; `sandbox/**`; `crates/harness-core/AGENTS.md`; Todo 3 scope taxonomy.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-core --test workspace_worktree_trust_vcs_sandbox_parity_test`; isolated worktrees have distinct roots/logs, trust precedes mutation, attribution survives restart, and supported sandbox/network denial is externally observed.
  QA scenarios: Happy: trust folder, create/select worktree, edit, inspect attribution, revert, and run allowed command. Failure: trust deny, path escape, symlink escape, writer collision, unsupported platform profile, and denied network/write leave workspace unchanged with actionable events. Evidence `<attemptDir>/task-19-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 20. Prove and complete tools, permissions, background tasks, scheduler, and teams
  What to do / Must NOT do: An `ultrabrain` async worker completes native tool lifecycle, permission-before-execution, remembered grants/always-approve boundaries, questions, task foreground/background/demotion/output/cancel, recurring scheduler fire/dedup/restart, team/subagent lifecycle/mailbox/progress, and event-derived owner receipts. Redelegation remains blocked where policy denies it. MUST NOT let TUI/CLI execute tools directly, fabricate tool completion, or resolve permissions outside coordinator authority.
  Parallelization: Wave 2 | Blocked by: 8,16 | Blocks: 24,29-32 | Exclusive writes: exact files generated from Todo 3 under `harness-core` permission/task/scheduler/team leaves and non-root native-tool modules; explicitly excludes `harness-tools/src/mcp*`, `code_lsp*`, integration modules, crate roots, and config paths
  References: `crates/harness-core/src/coord/AGENTS.md`; `crates/harness-tools/AGENTS.md`; `coord/task_lifecycle.rs`; `perm.rs`; `scheduler*`; `team_registry*`; `docs/native-tool-catalog.md`.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-core --test coord_test --test orchestration_scheduler_permission_parity_test`; `cargo nextest run -p harness-tools --test native_tool_parity_matrix_test`; permission events precede execution, cancellation terminates work, scheduled runs dedupe after restart, and child redelegation bypass mutations fail.
  QA scenarios: Happy: approve tool, answer question, background/demote/wait/cancel tasks, run recurring schedule, and complete team child. Failure: deny/timeout, duplicate schedule fire, unauthorized cancellation, worker redelegation, malformed tool output, and restart mid-task produce truthful terminal states. Evidence `<attemptDir>/task-20-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 21. Prove and complete local hooks, MCP, ACP, plugins, code graph, and LSP
  What to do / Must NOT do: An `ultrabrain` async worker implements file-discovered blocking hooks, local MCP stdio/configured HTTP lifecycle/tool discovery/restart, local ACP stdio agent mode, filesystem-local plugin discovery and trust, install from local directories, refresh/reload, enable/disable/uninstall, local-path marketplace source resolution, and observable activation of bundled agents/skills/hooks/MCP configuration; also implement persistent relationship-aware code graph and LSP diagnostics/symbols/references/install consent. Every visible integration state has a real filesystem/subprocess/transport owner and restart postcondition. MUST NOT add archive installation without new Todo 2 source-and-binary proof, remote/git/hosted marketplace fetching, remote OAuth/provisioning, binary plugin hosts, replay effects, or implicit network endpoints.
  Parallelization: Wave 2 | Blocked by: 8,16 | Blocks: 24,29-32 | Exclusive writes: exact integration/hook/MCP/code-LSP files generated from Todo 3; explicitly excludes all `harness-core/src/config/**`, provider/catalog files, native-tool files owned by Todo 20, and crate roots
  References: `crates/harness-tools/AGENTS.md`; `crates/harness-core/src/integrations/**`; `docs/config.md`; `docs/native-tool-catalog.md`; Todo 3 scope taxonomy.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-core --test local_integrations_parity_test`; `cargo nextest run -p harness-tools --test integrations_matrix_test --test native_tool_parity_matrix_test`; local plugin discover/trust/install/reload/enable/disable/uninstall/local-source and bundled-component activation tests exercise real filesystem/process postconditions and survive restart; replay remains side-effect free.
  QA scenarios: Happy: allow/block hooks, start/restart MCP, call first-class/generic MCP tools, run ACP stdio, discover/trust/install/reload/disable/enable/uninstall a local-directory plugin that contributes an agent, skill, hook, and MCP config, resolve a local-path marketplace source, and query code graph/LSP. Failure: untrusted plugin, malformed manifest, duplicate component, archive or remote/git marketplace input, hook denial, child crash, malformed protocol, remote OAuth config, replay invocation, and collision fail safely. Evidence `<attemptDir>/task-21-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 22. Prove and complete public auth, providers/models, updates, and sleep/wake protection
  What to do / Must NOT do: An `ultrabrain` async worker completes public Codex/Copilot/API-key credential flows, redacted storage/refresh, provider protocol/model switching/effort, streaming/reasoning/tool-call errors, binary update check/download/hash/apply/rollback/restart, and one process-scoped cross-platform sleep/wake credential supervisor. Retire enterprise/generic OIDC credentials atomically. MUST NOT persist secrets/raw requests, add enterprise surfaces, run telemetry, or spawn per-session power monitors.
  Parallelization: Wave 2 | Blocked by: 8,16 | Blocks: 24,28-32,37 | Exclusive writes: exact auth/provider runtime/update/sleep-wake files generated from Todo 3 and non-root provider transports; explicitly excludes `harness-core/src/config/**`, model/provider catalog configuration, crate roots, and CLI config leaves
  References: `crates/harness-core/src/auth.rs`; `sleep_wake_auth/**`; provider `AGENTS.md`; `crates/harness-providers/src/**`; `docs/provider-support.md`; Todo 3 exclusion ledger.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-providers`; `cargo nextest run -p harness-core --test auth_update_sleep_wake_parity_test`; persisted artifacts pass secret scan; update rollback restores prior binary; sleep prevents unsafe refresh and wake resumes exactly once.
  QA scenarios: Happy: login/refresh, switch model/effort, stream reasoning/tool result, apply verified update, sleep/wake refresh. Failure: invalid/rate-limited credential, protocol error, checksum mismatch, interrupted update, dark wake, retired enterprise record, and missing platform backend fail safely. Evidence `<attemptDir>/task-22-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 23. Prove and complete CLI, configuration/settings, doctor, and support export
  What to do / Must NOT do: A `deep` worker completes CLI leaf commands and `CliIo`/`CliDeps` paths for run/prompt/headless, sessions, worktrees, memory, queue, integrations, updates, config validate/show/sources/explain/settings, local doctor, and replay-derived support export. Rework typed runtime/TUI settings layering, migrations, schema generation, and write-back while preserving separate `harness.json{,c}` and `tui.json{,c}` contracts. MUST NOT put runtime authority in CLI, make doctor/network calls, expose secrets, or edit top-level command/root registries reserved to Todo 24.
  Parallelization: Wave 2 | Blocked by: 8,16 | Blocks: 24,25-32 | Exclusive writes: all exact `harness-core/src/config/**` and provider/model catalog configuration files from Todo 3, non-root CLI/support-export leaves, config/docs/schema tests; receives requirements from Todos 20-22 but is the only config/catalog writer
  References: `crates/harness/AGENTS.md`; `crates/harness-core/src/config/AGENTS.md`; `configs/AGENTS.md`; `docs/AGENTS.md`; `README.md`; `docs/config.md`.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness --test config_schema_cli_test --test config_docs_reference_test --test cli_contract_matrix_test --test support_export_test`; examples validate; effective config and explain attribution are deterministic/redacted; doctor performs no network/provider execution.
  QA scenarios: Happy: validate layered configs, mutate one typed setting, run local doctor, execute headless mock, inspect/export session. Failure: invalid schema, deprecated-only key, secret field, unavailable optional integration, corrupt session, and network canary in doctor fail closed. Evidence `<attemptDir>/task-23-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 24. Integrate retained runtime owners across crates
  What to do / Must NOT do: The sole Wave 2 `deep` integrator verifies Todos 17-23, wires public modules/registries/Cargo dependencies, updates events/docs/schemas/tool catalogs only where required, and resolves cross-crate compilation and contract mismatches. Preserve coordinator as sole scheduling/event authority and replay purity. MUST NOT rewrite leaf behavior, promote manifests, or reintroduce excluded families.
  Parallelization: Wave 2 serial integration gate | Blocked by: 17-23 | Blocks: 25-32 | Exclusive writes: workspace/crate manifests and roots, CLI/tool/provider registries, public config/docs schemas
  References: all crate `AGENTS.md`; Todo 8 reservations; Todos 17-23 receipts; root UPDATE TOGETHER table.
  Acceptance criteria (agent-executable): `cargo fmt --all -- --check`; `cargo check --workspace`; `cargo clippy --all-targets --all-features --workspace -- -D warnings`; targeted tests from Todos 17-23; `scripts/test-lanes.sh fast`; `scripts/test-lanes.sh integration`; `scripts/test-lanes.sh simulation`.
  QA scenarios: Happy: run a real mock session through config→auth/model→permission→tool→queue/interject→compaction→session replay/export in an isolated workspace. Failure: denied permission, tool crash, provider error, cancelled background work, and corrupt restart remain replayable and secret-clean. Run `bash scripts/harness-qa-dogfood.sh --slug retained-runtime`. Evidence `<attemptDir>/task-24-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 25. Reimplement startup, welcome, trust, and first-prompt journeys
  What to do / Must NOT do: A `visual-engineering` worker builds the measured bordered welcome panel, Harness identity/title/version, changelog/local notices, new/resume/worktree actions, folder-trust prompt, contextual warning/breadcrumb, first-prompt composer, and type-to-dismiss transition. Startup actions call real session/trust owners from Todos 17,19,23. MUST NOT restore compose-first startup, expose hosted/auth-excluded calls, edit overlay/session-picker roots, or read raw reference source.
  Parallelization: Wave 3 | Blocked by: 16,19,23,24 | Blocks: 32,33-35 | Exclusive writes: startup/welcome app/view leaves, `ui_lifecycle.rs`, trust prompt surface, startup tests
  References: Todo 8 startup behavior rows; `crates/harness-tui/src/app/lifecycle.rs`; `ui_lifecycle.rs`; fresh startup captures O-007/O-011; identity policy.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test startup_welcome_trust_parity_test`; exact semantic/raster/input agreement at 120x32 plus required responsive viewports; first input dismisses welcome and preserves the typed text/cursor.
  QA scenarios: Happy: launch first run, grant/deny trust, type first prompt, start new session and open resume/worktree action. Failure: missing auth/config, trust deny, tiny terminal, resize during dismiss, and Escape/focus sequences remain stable. Evidence `<attemptDir>/task-25-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 26. Reimplement main shell lifecycle, status, context, footer, and recovery states
  What to do / Must NOT do: A `visual-engineering` worker builds event-derived idle, streaming, permission, question, cancel, fail, recover, complete, post-run, and replay shell states; status/context/credit-free local bars; model/effort/context usage; dynamic shortcut footer; turn status; and handoff actions. Real runtime owners supply every state. MUST NOT show excluded billing/usage, persistent sidebars, invented statuses, or mutable controls in replay.
  Parallelization: Wave 3 | Blocked by: 16,17,20,22,24 | Blocks: 32,33-35 | Exclusive writes: shell-state/status/context/footer/post-run leaves and tests; no top-level `ui.rs`/`ui_chrome.rs`
  References: `crates/harness-tui/src/view_model.rs`; `app/activity.rs`; `app/lifecycle.rs`; Todo 8 SHELL behavior rows; Todos 17,20,22 owner receipts.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test shell_lifecycle_status_parity_test`; reference/candidate cells, focus, cursor, footer vocabulary, ordered states, and recovery transitions agree for every lifecycle row.
  QA scenarios: Happy: idle→stream→permission→tool→complete→post-run and replay. Failure: provider fail, cancel, permission timeout, recovery retry, and truncated/corrupt replay show truthful recoverable states without panic. Evidence `<attemptDir>/task-26-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 27. Reimplement transcript blocks, tools, diffs, markdown, links, and local media
  What to do / Must NOT do: A `visual-engineering` worker rebuilds user/assistant/thinking/system/session/context/compaction/background/subagent blocks, tool queued/permission/running/success/failure sections, edits/diffs, bash/read/search/list/other tool anatomy, markdown/tables/fences/syntax, hyperlinks, copy/meta viewer, Unicode/long content, Mermaid, and local inline image placement. Source content remains event-derived; view fold/selection state remains ephemeral. MUST NOT include hosted media generation or copy reference render code/theme tables.
  Parallelization: Wave 3 | Blocked by: 12,17,18,20-22,24 | Blocks: 32,33-35 | Exclusive writes: `ui_transcript_*`, `ui_tool_*`, `ui_diff*`, markdown/fenced/syntax/media leaves except top-level `ui_transcript.rs`
  References: `crates/harness-tui/src/ui_transcript.rs` module map; `app/tool_call.rs`; Todo 8 TX behavior rows; Todo 12 scrollback contract; owner receipts from Todos 17-22.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test transcript_block_tool_diff_media_parity_test --test reference_parity_tx_transcript_test`; cells/raster/frame traces cover streaming/completed/failed tool, edit diff, markdown, syntax, link, selection, clipboard, media, long Unicode, and compaction.
  QA scenarios: Happy: mixed transcript with reasoning, tools, diff, table, code, links, Mermaid, image, background and subagent completion. Failure: malformed markdown/media, huge output truncation, failed edit/tool, missing artifact, and resize during stream remain safe and truthful. Evidence `<attemptDir>/task-27-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 28. Reimplement overlays, pickers, settings, permissions, questions, and local integrations UI
  What to do / Must NOT do: A `visual-engineering` worker implements command palette, slash/argument/file completion overlays, session/rewind/model/effort pickers, permission and question queues/follow-up input, settings browse/filter/edit/reset, auth/public credential dialog, memory browser, MCP/local integrations/extensions, prompt stash, details/status, and block/doc viewers. Every action routes through Todo 15 and real owners. Theme dialog/notification surfaces belong to Todo 31. MUST NOT expose excluded commands/settings or let modal state mutate runtime directly.
  Parallelization: Wave 3 | Blocked by: 13,15,17,20-24 | Blocks: 32,33-35 | Exclusive writes: `ui_overlays/**` except theme/notification-owned leaves, picker/modal app leaves, overlay tests; no top-level `ui_overlays.rs`
  References: `crates/harness-tui/src/ui_overlays.rs` module map; `app/permissions.rs`; `app/model_switcher.rs`; `app/secondary_surfaces.rs`; Todo 8 11-modal/64-command/40-setting inventory.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test overlay_picker_settings_permission_parity_test`; all in-scope commands/settings are discoverable and executable, all excluded members are absent, focus/escape/tab/mouse behavior matches, and owner postconditions are observed.
  QA scenarios: Happy: palette→model, settings edit/revert, permission allow/deny/follow-up, question select/text, resume/rewind, memory/MCP/extensions inspect. Failure: unavailable owner, restricted command, invalid setting, empty permission draft, stacked modal escape, and replay mutation attempt fail safely. Evidence `<attemptDir>/task-28-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 29. Reimplement plan, vim, minimal, inline, fullscreen, rewind, and mode transitions
  What to do / Must NOT do: A `visual-engineering` worker implements local plan workflow/view/approval/resume, simple/vim navigation, compact/minimal native-scrollback mode, inline/fullscreen switching, fold/raw/manual preferences, and rewind/stash transitions. Mode state must preserve prompt/session/runtime ownership and terminal restoration. MUST NOT create competing primary shells, permit plan writes outside the allowed plan artifact, or make replay mutating.
  Parallelization: Wave 3 | Blocked by: 9,10,13,15,17,18,24 | Blocks: 32,33-35 | Exclusive writes: plan/mode/rewind view-state leaves, minimal/inline/fullscreen adapters, mode tests
  References: Todo 8 plan/vim/screen-mode commands/settings; `crates/harness-tui/src/app/composer.rs`; plan view overlay; terminal lifecycle from Todo 10; session rewind owner from Todo 17.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test plan_vim_screen_mode_parity_test`; matched action/state/frame traces for plan enter/edit/view/approve/exit, vim/simple navigation, minimal/inline/fullscreen relaunch, rewind and prompt stash.
  QA scenarios: Happy: enter plan, write approved plan path, approve, resume build; toggle vim and screen modes; rewind and restore draft. Failure: unauthorized plan path, cancel/reject approval, unsupported terminal mode, child suspend failure, and replay mode mutation are rejected/restored. Evidence `<attemptDir>/task-29-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 30. Reimplement dashboard, queue/tasks/todo, subagents, and worktree navigation
  What to do / Must NOT do: A `visual-engineering` worker implements multi-session dashboard roster/grouping/pin/rename/reorder/stop/auto-approve/location/worktree actions; queue/tasks/todo panes; child/subagent status/catalog/details; session/worktree entry and return; and background completion navigation. Use replay/live projections from Todos 17-20. MUST NOT invent a second scheduling authority, expose remote workspace actions, or edit runtime owner state directly.
  Parallelization: Wave 3 | Blocked by: 15,17-20,24 | Blocks: 32,33-35 | Exclusive writes: dashboard and secondary pane leaves, session/worktree navigation UI leaves, dashboard tests
  References: `crates/harness-tui/src/app/session_stack.rs`; `app/secondary_surfaces.rs`; `ui_secondary*`; Todo 8 dashboard action inventory; owner receipts from Todos 17-20.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test dashboard_task_queue_worktree_parity_test`; dashboard commands have real session/task/worktree postconditions, queue/tasks/todo state is event-derived, and navigation survives restart/replay.
  QA scenarios: Happy: create multiple sessions/worktrees, group/pin/rename/reorder, open child, background/cancel task, enqueue/send-now, inspect todo, stop and return. Failure: stale session, active writer lock, cancelled child, removed worktree, unauthorized auto-approve, and empty dashboard recover without corruption. Evidence `<attemptDir>/task-30-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 31. Reimplement local notifications, tips, appearance preview, and diagnostics surfaces
  What to do / Must NOT do: A `visual-engineering` worker implements local task/subagent/tool/update/focus notifications, contextual tips/hints, theme picker preview/apply/revert and auto system appearance, terminal capability diagnostics/setup guidance, FPS/scroll debug only behind explicit debug controls, and sleep/wake/focus-aware notification timing. MUST NOT fetch hosted announcements/release notes/usage, emit telemetry, or persist preview values before apply.
  Parallelization: Wave 3 | Blocked by: 10,14,20,22-24 | Blocks: 32,33-35 | Exclusive writes: notification/tip/theme-dialog/terminal-diagnostics leaves and tests
  References: Todo 8 setting/command inventory; `crates/harness-tui/src/theme.rs`; `ui_overlays/theme_dialog.rs`; runtime notifications; Todo 22 sleep/wake/update receipts.
  Acceptance criteria (agent-executable): `cargo nextest run -p harness-tui --test notification_theme_diagnostics_parity_test`; local notification ordering/dismissal, tips, preview/revert, auto dark/light, color capability adaptation, and diagnostics match contracts without network calls.
  QA scenarios: Happy: task completion while unfocused, permission focus alert, theme preview/revert/apply, system appearance switch, terminal diagnostic. Failure: notification storm, focus race, sleep transition, unsupported color/clipboard/mouse, and hosted command attempt remain bounded/absent. Evidence `<attemptDir>/task-31-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 32. Integrate all TUI surfaces with retained runtime owners
  What to do / Must NOT do: The sole Wave 3 `visual-engineering` integrator reviews Todos 25-31, wires top-level `app.rs`, `ui.rs`, `ui_transcript.rs`, `ui_overlays.rs`, keybindings/overlay registries, module exports, and TUI manifest ownership. Resolve integration only; route leaf defects back. Ensure all 74 in-scope/excluded action dispositions and 64 command instances are accounted for. MUST NOT promote status or add a visual-only no-op action.
  Parallelization: Wave 3 serial integration gate | Blocked by: 24-31 | Blocks: 33-39 | Exclusive writes: TUI shared roots, action/keybinding/overlay registries, TUI Cargo manifest, behavior ownership fields
  References: Todos 25-31 receipts; Todo 8 inventories/manifests; `crates/harness-tui/AGENTS.md`; `DESIGN.md`.
  Acceptance criteria (agent-executable): `cargo fmt --all -- --check`; `cargo check -p harness-tui -p harness`; all Wave 1/3 targeted tests; `cargo nextest run -p harness-tui`; `scripts/test-lanes.sh signoff-journeys` with explicit candidate binary and fresh evidence root; no manifest status promotion.
  QA scenarios: Happy: real binary journey from welcome through session/tool/queue/plan/dashboard/settings/replay/exit at 80x24 and 120x32. Failure: bad config, denied trust/permission, provider/tool failure, cancel/recover, resize storm, focus CSI, corrupt resume, and unsupported terminal profile. Use Playwright/xterm capture tooling and `visual-qa` review; run dogfood `--slug tui-surfaces`. Evidence `<attemptDir>/task-32-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 33. Generate independent differential scenario and holdout drivers
  What to do / Must NOT do: A `deep` QA worker generates behavior-descriptive Harness scenario specifications and drivers from the canonical inventory/contracts, covering every row and proof dimension. Scenarios specify initial state, input bytes/actions, checkpoints, focus/cursor, expected owner/postcondition, viewport/capability profile, timing window, failure mutation, and teardown. Split published conformance scenarios from undisclosed holdouts. MUST NOT copy reference YAML/tests/fixtures or read raw reference source.
  Parallelization: Wave 4 | Blocked by: 32 | Blocks: 34-38 | Can run with: none | Exclusive writes: new clean-room scenario schema/drivers and QA tests
  References: Todo 8 canonical inventories; P0-P9 proof vector; `crates/harness-testkit/AGENTS.md`; existing PTY/scenario helpers as Harness patterns only.
  Acceptance criteria (agent-executable): `python3 scripts/parity_task_qa.py validate-scenarios --inventory docs/grok-reference-interaction-inventory.v1.json --scenario-root <attemptDir>/task-33-grok-build-clean-room-parity/scenarios --holdout-index <attemptDir>/task-33-grok-build-clean-room-parity/holdout-index.json --output <attemptDir>/task-33-grok-build-clean-room-parity/scenario-validation.json --require-happy 1 --require-failure 1 --require-mutation 1 --require-coverage 100` exits 0 and prints `coverage=100% missing=0 copied_reference_assets=0`; every action/command/setting/modal/terminal family is covered and excluded rows have absence/rejection cases.
  QA scenarios: Happy: generate and execute a representative static, interaction, lifecycle, owner, responsive, and timing scenario on both binaries. Failure: missing row, copied reference fixture hash, grouped wildcard scenario, absent teardown, or no failure mutation is rejected. Evidence `<attemptDir>/task-33-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 34. Execute semantic-cell, raster, responsive, color, and terminal-capability differential proof
  What to do / Must NOT do: An independent `visual-engineering` QA worker runs exact reference/candidate binaries through matched fresh environments for all static/settled visual rows and viewport/capability profiles. Capture PTY terminal text, semantic cells, cursor/alternate state, xterm raster, and native raster where available; compare with identity-only substitutions. MUST NOT repair product code, widen masks, reuse a prior artifact, or mark native-unavailable rows pass.
  Parallelization: Wave 4 | Blocked by: 32,33 | Blocks: 38 | Can run with: 35-37 | Global locks: REFERENCE_CAPTURE, CANDIDATE_BINARY, PTY_NATIVE_DISPLAY
  References: `scripts/tui-parity/**`; Todo 6 comparator; viewport matrix in Todo 14; terminal profiles in Todo 9; canonical visual rows.
  Acceptance criteria (agent-executable): Recompute and match `reference_epoch` before and after capture; every applicable row has cross-source P3/P4 receipts; zero unapproved cell/RGBA differences; dimensions, colors, modifiers, cursor, focus, alternate-screen, and identity bounds validate. A missing native prerequisite records a temporary blocker and prevents final release until rerun passes.
  QA scenarios: Happy: startup/live/transcript/overlay/dashboard/mode/theme captures across all required widths and color profiles. Failure: one-cell geometry, wrong color, cursor/focus, self-oracle, helper binary, mask expansion, or stale screenshot is detected. Evidence `<attemptDir>/task-34-grok-build-clean-room-parity/`.
  Commit: N | Independent QA task.

- [ ] 35. Execute ordered motion, timing, scroll, resize, streaming, and cancellation proof
  What to do / Must NOT do: An independent `visual-engineering` QA worker records synchronized frame sequences and monotonic timestamps for animation demand, spinner/accent/finish flash, cursor stability, streaming deltas, scroll flush/finalize, follow mode, resize debounce, overlay transition, cancellation, recovery, and settle dwell. Compare state ordering and bounded timing contracts, not a single final frame. MUST NOT hide nondeterminism with broad tolerances or historical divergence receipts.
  Parallelization: Wave 4 | Blocked by: 32,33 | Blocks: 38 | Can run with: 34,36,37 | Global locks: REFERENCE_CAPTURE, CANDIDATE_BINARY, PTY_NATIVE_DISPLAY
  References: Todo 10 clocks; Todo 12 scrollback; Todo 26 lifecycle; P5 proof contract; reference timing/scroll behavior contracts generated in Todo 8.
  Acceptance criteria (agent-executable): Recompute and match `reference_epoch` before and after capture; ordered event/frame traces match required transitions and timing windows; settle requires repeated identical frames; no input starvation, cursor churn, skipped cancel state, or resize corruption.
  QA scenarios: Happy: streaming tool turn with scroll-away/follow recovery, resize burst, finish flash, cancel and retry. Failure: injected delayed/missing/reordered frame, excessive settle, dropped input, raw focus bytes, or stale spinner state is rejected. Evidence `<attemptDir>/task-35-grok-build-clean-room-parity/`.
  Commit: N | Independent QA task.

- [ ] 36. Execute owner postcondition, persistence, restart, error, and replay holdouts
  What to do / Must NOT do: An independent `unspecified-high` QA worker drives every retained action through the compiled public surface and observes real owner-side effects: events, files, processes, transports, credentials, sessions, tasks, worktrees, settings, exports, updates, and teardown. Repeat critical journeys across restart/replay and inject failures. MUST NOT accept a registry/help/diagnostic/mock-only result when the row claims product behavior.
  Parallelization: Wave 4 | Blocked by: 24,32,33 | Blocks: 38 | Can run with: 34,35,37 | Global locks: CANDIDATE_BINARY, CONFIG_INTEGRATION_REGISTRY
  References: Todos 17-24 owner contracts; Todo 20 coordinator invariants; P2/P7 proof dimensions; canonical journey rows.
  Acceptance criteria (agent-executable): Every retained visible action has a compiled owner receipt and expected external postcondition; replay executes no effects; restart preserves or truthfully recovers state; failures create terminal events without unauthorized mutation.
  QA scenarios: Happy: sessions, queue/interject, memory/compaction, worktree/trust, tool/task/team, MCP/ACP/hooks/LSP, auth/model/update, config/doctor/export. Failure: deny/timeout/crash/corruption/network absence/checksum mismatch/cancellation/lock contention. Evidence `<attemptDir>/task-36-grok-build-clean-room-parity/`.
  Commit: N | Independent QA task.

- [ ] 37. Execute live-provider, installed-binary, native dogfood, secret, and clean-room holdouts
  What to do / Must NOT do: An independent `unspecified-high` QA worker builds/installs an explicit candidate binary, resolves current effective live provider/model configuration, runs one live journey per distinct backend model id plus alias-resolution tests, offline dogfood journeys, native terminal/clipboard proof when available, evidence secret scans, and clean-room structural/path audits. Credentials are injected only into the parent process and stripped from child/reference environments. MUST NOT expose credentials, duplicate live runs for aliases, or treat unavailable live/native environments as pass.
  Parallelization: Wave 4 | Blocked by: 22-24,32,33 | Blocks: 38 | Can run with: 34-36 | Global locks: LIVE_PROVIDER, CANDIDATE_BINARY, PTY_NATIVE_DISPLAY
  References: `harness.jsonc`; `scripts/harness-qa-dogfood.sh`; `scripts/test-lanes.sh signoff-live/signoff-native`; `crates/harness-testkit/src/secret_scanner.rs`; clean-room contract from Todo 7.
  Acceptance criteria (agent-executable): `harness config show --effective` determines provider/model matrix; each distinct configured backend proves auth, streaming, tool/permission, cancel/error/recovery; aliases prove resolution; evidence/child env scans are clean; copied-reference/path similarity holdouts pass or return typed findings.
  QA scenarios: Happy: installed offline first-run and configured live prompt/tool journey, native clipboard/visual capture, support export. Failure: missing credential/native display becomes `blocked`; canary secrets never reach child/reference/evidence; copied identifier/source fragment or reference path in product tests is rejected. Evidence `<attemptDir>/task-37-grok-build-clean-room-parity/`.
  Commit: N | Independent QA task.

- [ ] 38. Integrate the complete evidence set and run deterministic rejection gates
  What to do / Must NOT do: The Wave 4 `deep` evidence integrator validates Todos 33-37, wires final scenario/lane/schema changes through shared scripts/test roots, computes completeness without promoting statuses, and runs all stale/copy/self-oracle/wrong-binary/secret/mask/scope-resurrection/owner-bypass/scheduler mutations. Route any product defect back to its earliest owner; evidence code may be fixed here only when the product is unchanged. MUST NOT combine epochs or rewrite observed artifacts.
  Parallelization: Wave 4 integration gate | Blocked by: 33-37 | Blocks: 39 | Exclusive writes: evidence/test shared roots and lane scripts; global locks EVIDENCE_EPOCH and FULL_GATE_RUNNER
  References: Todos 33-37 receipts; Todo 4 schema; Todo 5 lane ownership; Todo 7 scheduler; public manifests.
  Acceptance criteria (agent-executable): `scripts/test-lanes.sh quality-gates`; `scripts/test-lanes.sh all-deterministic`; `scripts/test-lanes.sh simulation`; parity artifact/scheduler/identity mutation suites; completeness report shows every in-scope row has its required proof vector or a typed external blocker, and all statuses remain incomplete.
  QA scenarios: Happy: validate the full fresh evidence tree. Failure: remove one artifact, alter one digest, mix one epoch, swap binary, copy one old screenshot, resurrect one excluded command, or bypass one owner; each mutation fails with the correct typed reason. Evidence `<attemptDir>/task-38-grok-build-clean-room-parity/`.
  Commit: N | Do not commit unless explicitly requested.

- [ ] 39. Seal one product epoch, build the candidate, and produce final acceptance evidence
  What to do / Must NOT do: The lead only acquires locks and dispatches an independent `unspecified-high` attestation worker. That worker verifies no source writer remains active, computes `product_epoch`, builds with `CARGO_TARGET_DIR=<attemptDir>/task-39-grok-build-clean-room-parity/sealed-target`, computes the binary SHA, installs it as read-only `<attemptDir>/task-39-grok-build-clean-room-parity/candidate/<sha256>/harness`, and reruns every applicable proof using only that immutable path and the frozen reference. Every runner verifies candidate/reference path and SHA before and after execution. Create `attestation-inputs.json` with product/reference/evidence/manifest digests. The lead MUST NOT execute captures or write evidence/manifests. Any defect invalidates the seal and reopens the earliest owner.
  Parallelization: Wave 4 serial seal | Blocked by: 38 | Blocks: F1-F4 | Global locks: CANDIDATE_BINARY, REFERENCE_CAPTURE, EVIDENCE_EPOCH, FULL_GATE_RUNNER, LIVE_PROVIDER, PTY_NATIVE_DISPLAY
  References: `scripts/parity_epoch.py`; Todo 4 provenance; Todos 34-38 receipts; root build/test commands.
  Acceptance criteria (agent-executable): `CARGO_TARGET_DIR=<sealed-target> cargo build -p harness` succeeds; the installed content-addressed candidate is mode 0555 and unchanged before/after every reviewer; product/reference epochs recompute exactly; all receipts name one candidate SHA and frozen reference SHA/epoch; full format/check/clippy/deterministic/simulation/explicit-binary signoff/dogfood plus every applicable live/native row pass; zero in-scope rows remain blocked.
  QA scenarios: Happy: recompute every digest and rerun a random holdout against the sealed binary. Failure: one source byte changes after seal, one receipt names another binary/epoch, or one final command is skipped/nonzero; attestation is invalid. Evidence `<attemptDir>/task-39-grok-build-clean-room-parity/`.
  Commit: N | Read-only seal/acceptance task; any source fix reopens prior tasks.

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance and evidence audit
  Run `python3 scripts/run-parity-review.py prepare --kind plan-compliance --plan .omo/plans/grok-build-clean-room-parity.md --attestation <attemptDir>/task-39-grok-build-clean-room-parity/attestation-inputs.json --evidence-root <attemptDir> --output <attemptDir>/final/F1-input.json`. Then invoke `task(subagent_type="oracle", load_skills=[], run_in_background=false, description="Final plan compliance", prompt="Read <attemptDir>/final/F1-input.json and every referenced artifact. Verify every plan requirement, dependency, reservation, proof dimension, command, epoch, mutation, and receipt. Return strict JSON with verdict unconditional_approval|rejected and typed findings; do not edit files.")`. Validate the coordinator task receipt with `python3 scripts/run-parity-review.py validate-agent-receipt --kind plan-compliance --input <attemptDir>/final/F1-input.json --receipt <F1-task-receipt.json> --output <attemptDir>/final/F1-plan-compliance.json --require-verdict unconditional_approval`; all commands must exit 0.

- [ ] F2. Code quality, architecture, security, and replay audit
  Run `CARGO_TARGET_DIR=<attemptDir>/final/review-target-F2 cargo clippy --all-targets --all-features --workspace -- -D warnings` and targeted nextest holdouts without touching the immutable candidate. Run `python3 scripts/run-parity-review.py prepare --kind code-security --plan .omo/plans/grok-build-clean-room-parity.md --attestation <attemptDir>/task-39-grok-build-clean-room-parity/attestation-inputs.json --evidence-root <attemptDir> --output <attemptDir>/final/F2-input.json`. Invoke `task(subagent_type="oracle", load_skills=[], run_in_background=false, description="Final code security review", prompt="Read <attemptDir>/final/F2-input.json and the complete current diff. Audit coordinator/event/replay/permission/cancellation/writer-lock/path/redaction/secret/async/platform/dependency/test invariants. Return strict JSON verdict and typed findings; do not edit.")`. Validate with `python3 scripts/run-parity-review.py validate-agent-receipt --kind code-security --input <attemptDir>/final/F2-input.json --receipt <F2-task-receipt.json> --output <attemptDir>/final/F2-code-security.json --require-verdict unconditional_approval`.

- [ ] F3. Real manual TUI/CLI/API QA and visual fidelity review
  Run `python3 scripts/run-parity-review.py prepare --kind manual-visual --plan .omo/plans/grok-build-clean-room-parity.md --attestation <attemptDir>/task-39-grok-build-clean-room-parity/attestation-inputs.json --candidate <attemptDir>/task-39-grok-build-clean-room-parity/candidate/<sha256>/harness --reference "$REFERENCE_BIN" --evidence-root <attemptDir> --output <attemptDir>/final/F3-input.json`. Invoke `task(category="visual-engineering", load_skills=["frontend","visual-qa","playwright"], run_in_background=false, description="Final real visual QA", prompt="Read <attemptDir>/final/F3-input.json. Launch only the immutable candidate and frozen reference through real CLI/TUI/PTY/xterm/native surfaces. Drive every listed happy and bad-input holdout, capture cells/raster/frame timing/focus/cursor/mouse/clipboard/Unicode/resize/teardown, and return strict JSON verdict and typed findings. Do not edit product or masks.")`. Validate with `python3 scripts/run-parity-review.py validate-agent-receipt --kind manual-visual --input <attemptDir>/final/F3-input.json --receipt <F3-task-receipt.json> --output <attemptDir>/final/F3-manual-visual.json --require-verdict unconditional_approval`.

- [ ] F4. Scope fidelity and clean-room audit
  Run `python3 scripts/run-parity-review.py prepare --kind scope-cleanroom --plan .omo/plans/grok-build-clean-room-parity.md --attestation <attemptDir>/task-39-grok-build-clean-room-parity/attestation-inputs.json --inventory docs/grok-reference-interaction-inventory.v1.json --scope docs/grok-cleanroom-scope.v1.json --removals docs/scope-removal-ledger.v1.json --evidence-root <attemptDir> --output <attemptDir>/final/F4-input.json`. Invoke `task(subagent_type="oracle", load_skills=[], run_in_background=false, description="Final scope clean-room review", prompt="Read <attemptDir>/final/F4-input.json, current diff/history, reference-path and structural-similarity reports. Prove complete local scope, approved exclusions, identity-only substitution, no inherited statuses/divergences/task numbering, and no copied source/test/fixture/evidence. Return strict JSON verdict and typed findings; do not edit.")`. Validate with `python3 scripts/run-parity-review.py validate-agent-receipt --kind scope-cleanroom --input <attemptDir>/final/F4-input.json --receipt <F4-task-receipt.json> --output <attemptDir>/final/F4-scope-cleanroom.json --require-verdict unconditional_approval`.

- [ ] F5. Propose final attestation and status promotion without writing manifests
  After F1-F4 approve, the lead only acquires `MANIFEST_PROMOTION`/`EVIDENCE_EPOCH` and dispatches `task(category="unspecified-high", load_skills=["karpathy-guidelines"], run_in_background=false, description="Propose final parity promotion", prompt="Read F1-F4 approvals, sealed attestation inputs, canonical manifests, and full evidence. Do not edit manifests. Produce strict JSON proposed row changes, unchanged rows, zero blockers, zero unapproved divergences, input/output manifest digests, product/reference epochs, and rationale at <attemptDir>/final/F5-proposed-promotion.json.")`. Validate the task receipt and proposal with `python3 scripts/run-parity-review.py validate-promotion-proposal --proposal <attemptDir>/final/F5-proposed-promotion.json --reviews <attemptDir>/final/F1-plan-compliance.json <attemptDir>/final/F2-code-security.json <attemptDir>/final/F3-manual-visual.json <attemptDir>/final/F4-scope-cleanroom.json --attestation <attemptDir>/task-39-grok-build-clean-room-parity/attestation-inputs.json --require-zero-blocked --require-zero-divergence`; canonical manifests remain unchanged.

- [ ] F6. Terminal Oracle release-stop review
  Run `python3 scripts/run-parity-review.py prepare --kind terminal-oracle --plan .omo/plans/grok-build-clean-room-parity.md --attestation <attemptDir>/task-39-grok-build-clean-room-parity/attestation-inputs.json --reviews <attemptDir>/final/F1-plan-compliance.json <attemptDir>/final/F2-code-security.json <attemptDir>/final/F3-manual-visual.json <attemptDir>/final/F4-scope-cleanroom.json --proposal <attemptDir>/final/F5-proposed-promotion.json --output <attemptDir>/final/F6-input.json`. Invoke one fresh `task(subagent_type="oracle", load_skills=[], run_in_background=false, description="Terminal parity release review", prompt="Read <attemptDir>/final/F6-input.json and all referenced sealed artifacts. Decide whether the proposed promotion proves complete 1:1 local parity. Return strict JSON verdict unconditional_approval|rejected with typed findings and the exact approved proposal SHA; do not edit.")`. Validate with `python3 scripts/run-parity-review.py validate-agent-receipt --kind terminal-oracle --input <attemptDir>/final/F6-input.json --receipt <F6-task-receipt.json> --output <attemptDir>/final/F6-oracle.json --require-verdict unconditional_approval --require-proposal-sha <proposal-sha>`. Rejection reopens the earliest named owner and invalidates affected epochs/evidence.

- [ ] F7. Apply the Oracle-approved promotion mechanically
  F7 is the named final-attestation integrator and sole serial-root exception authorized to reserve/write `docs/tui-reference-parity-manifest.v1.json` and `docs/capability-inventory.v1.json`. The lead dispatches an independent `deep` worker with `karpathy-guidelines` and `programming`. The worker is authorized to run this fixed command list and no other mutating command: (1) `python3 scripts/run-parity-review.py apply-promotion --proposal <attemptDir>/final/F5-proposed-promotion.json --oracle <attemptDir>/final/F6-oracle.json --attestation <attemptDir>/task-39-grok-build-clean-room-parity/attestation-inputs.json --tui-manifest docs/tui-reference-parity-manifest.v1.json --capability-manifest docs/capability-inventory.v1.json --output <attemptDir>/final/F7-applied-promotion.json`; (2) pre/post `sha256sum <immutable-candidate>`; (3) `HARNESS_BIN=<immutable-candidate> HARNESS_TUI_PARITY_ARTIFACT_DIR=<attemptDir>/final/F7-signoff-parity scripts/test-lanes.sh signoff-parity`; (4) `cargo nextest run -p harness-tui --test reference_parity_manifest_test --test reference_parity_evidence_test`; (5) `python3 scripts/run-parity-review.py finalize-attestation --applied <attemptDir>/final/F7-applied-promotion.json --oracle <attemptDir>/final/F6-oracle.json --candidate <immutable-candidate> --signoff-root <attemptDir>/final/F7-signoff-parity --output <attemptDir>/final/F7-attestation.json`. `apply-promotion` is the only permitted mutating command. `finalize-attestation` runs last and binds candidate pre/post SHA, applied manifest digests, every command/exit status, F7 artifact hashes, and F5/F6 identities. Canonical output digests must equal the F5 proposal.

## Commit strategy
- Default: no commits, staging, amendments, pushes, or PRs. The user did not request Git publication.
- If a later execution instruction explicitly requests commits, use one atomic commit per completed integration gate after its full verification, never per leaf worker. Inspect status/diff/log before each commit and stage only the gate's reserved paths.
- Never commit evidence roots, secrets, sessions, runtime artifacts, reference binaries/source, or historical attempt trees.
- Suggested gate commit subjects if explicitly authorized: `test(parity): establish clean-room evidence foundation`, `feat(tui): align terminal and render primitives`, `feat(runtime): complete retained local owners`, `feat(tui): reproduce local Grok journeys`, `test(parity): seal differential acceptance evidence`.

## Success criteria
- Starting-state inventory covers every original dirty path, and no unrelated/user path was lost, reset, hidden, copied, or modified outside its approved classification/reservation.
- Frozen reference path, revision, version, SHA, and clean status match this plan; every reference capture used the exact binary.
- Canonical inventories regenerate without drift and cover every action, action definition, builtin command/module, setting, modal, terminal profile, runtime feature owner, and documented/observed local journey.
- Every retained local behavior row is `pass`; zero in-scope rows are `blocked`. If a prerequisite cannot be supplied, execution remains incomplete or the user must request a new reduced-scope plan that cannot claim complete 1:1 parity. No internal implementation gap is `blocked`, and no divergence exists without new user approval.
- Every pass is bound to one product epoch, one Harness binary SHA, the frozen reference SHA, complete applicable P0-P9 evidence, real compiled owner/postcondition, failure mutations, secret-clean artifacts, and successful teardown.
- All approved excluded families are absent from public config, commands, actions, settings, network paths, and runtime behavior; historical persisted records replay/retire side-effect free with actionable errors.
- Harness identity is the only substitution. Geometry, colors, modifiers, content behavior, focus, cursor, responsive layout, terminal capability behavior, timing, animation, scroll, resize, error/cancel/recovery, and side effects match the reference contracts.
- Coordinator/event/replay/permission/redaction/writer-lock invariants hold; replay executes no provider/tool/hook/MCP/network/CLI work.
- Workspace format/check/clippy, targeted owner tests, all deterministic lanes, simulation, explicit-binary binary/PTY/parity/journey signoff, offline dogfood, and applicable live/native lanes pass on the sealed candidate.
- F1-F4 return unconditional approval, F5 produces a valid zero-blocker proposal without manifest writes, F6 returns unconditional approval for the exact proposal, and F7 applies it mechanically with no unrelated byte changes.
- The user receives the final report and explicitly accepts completion; until then execution remains complete-but-unreleased.

## Plan review receipts
- Metis pre-draft gap analysis completed; reference access, dirty-tree taxonomy, local-scope definition, evidence/status contracts, reservations, epochs, lane ownership, final acceptance, and clean-room controls were resolved in this plan.
- Momus high-accuracy plan review: `OKAY`; session `ses_05f2b61f4ffeGxk4jiDe9BulrO`.
- Independent Oracle plan review: `unconditional_approval`; session `ses_05f2b61e4ffe4k5Xko5ifMqndI`.
- Residual non-blocking execution risks: live/native prerequisites can pause release; generated exact-file reservations override broad prose paths; native raster/timing evidence requires a stable host without tolerance expansion.

## Approved Clean-Room Exclusion Families for This Cycle

The following feature families are explicitly approved as out-of-scope for the current program cycle and are promoted to `excluded` status in the canonical manifests:

- reference-divergent-tui-tool-diff (TX-TOOL and TX-DIFF transcript tool/diff chrome divergence)
- subpixel-aa-overlay-session-picker (OVL-PALETTE sub-pixel AA pixel divergence and OVL-SESSION nondeterministic reference capture)
- enterprise-oidc (auth.browser_oidc_sso)
- remote-workspace-hub (remote.workspace_hub)
- remote-mcp-oauth (mcp.oauth_remote_transports)
- marketplace-hosted-share-media (plugins.marketplace_install and cli.share)
- windows-sandboxing-seatbelt (sandbox.seatbelt_windows)
- provider-live-proof-external (provider.non_openai_live_proof)

These families are retained in scope as documented but excluded from the pass-set by user-approved reduced-scope acceptance.

## Approved Implementation Patterns for This Cycle

- Linux Landlock sandbox helper and shell runner use unsafe Linux syscalls with explicit `#![allow(unsafe_code)]` justifications. These are platform-specific and not copied reference code.

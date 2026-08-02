# Grok Build Parity Parallel Execution Plan

> **Canonical plan:** this root file is the only execution plan for the next loop.
> Do not redirect to `.omo/plans/`, reuse an older checkpoint, or inherit any old
> completion claim. Every checkbox below starts unchecked.

## TL;DR

Bring every retained local Harness surface to behavioral and visual parity with
the pinned Grok Build source and executable under `inspirations/grok-build`.
“Parity” means the same observable actions, state transitions, timing,
shortcuts, focus, geometry, terminal cells, settled pixels, errors, recovery,
persistence, and side effects, except for explicitly allowed Harness branding.

The previous loop is not a completion baseline. It ended with F1/F2/F4 failures,
a dirty partially integrated worktree, false-success command shards, unreliable
manifest statuses, missing evidence bundles, and zero accepted TUI parity rows.
The next loop begins by preserving and classifying that work, removing excluded
features, repairing runtime authority and evidence provenance, then completing
and proving every retained reference-visible feature.

## 1. Locked product decisions

### 1.1 Scope IN

- Exact Grok Build look and feel for every retained reference-visible TUI state,
  interaction, mode, overlay, viewport, shortcut, terminal capability, and
  animation state.
- Harness-native event authority, replay purity, permission-before-execution,
  redaction, cancellation, persistence, and recovery semantics.
- Local sessions, fork/clone/rewind/recovery, memory, prompt queues,
  interjections, background tasks, teams, cron, worktrees, VCS attribution,
  folder trust, Linux sandboxing, hooks, plugins/extensions, ACP stdio, local MCP
  stdio and streamable HTTP, code intelligence, export/trace, update, settings,
  providers, models, doctor, and CLI workflows.
- TUI dashboard, settings, session/model/agent flows, plan mode, vim mode,
  minimal/fullscreen modes, file search and `@` completion, history/search,
  notifications, contextual tips, themes and auto/system theme behavior,
  inline image/media rendering, mouse, selection, clipboard, hyperlinks,
  responsive layouts, and reduced-terminal fallbacks.
- Native OS sleep/wake monitoring and credential-refresh protection. This is
  explicitly retained and must use real platform event sources rather than only
  an injectable test channel.
- Live acceptance through the enabled `umans-ai-coding-plan` provider for each
  unique underlying model currently declared in `harness.jsonc`:
  `umans-glm-5.2`, `umans-kimi-k2.7`, and `umans-qwen3.6-35b-a3b`.
  `umans-coder` is an alias of Kimi K2.7 and `umans-flash` is an alias of Qwen
  3.6 35B A3B; both aliases receive config/model-resolution coverage but no
  duplicate live-provider run.

The alias contract is explicit: the typed model resolver must map logical model
ID `umans-coder` to canonical backend model ID `umans-kimi-k2.7`, and
`umans-flash` to `umans-qwen3.6-35b-a3b`. Task 23 owns the resolver/catalog
contract and tests; Task 35 records logical ID, canonical backend ID, and wire
provider model ID in its receipt. No live call is accepted for an alias unless
the receipt proves it reached the declared canonical backend.

### 1.2 Scope OUT and Must-NOT-Have

- Voice capture, dictation, speech-to-text, text-to-speech, `/voice`, voice
  settings, voice status, voice tests, voice capability rows, or a Whisper/model
  dependency.
- Generic Enterprise SSO, generic browser OIDC, the `browser_oidc` and
  `browser_oidc_local` product surfaces, and GitHub Copilot Enterprise deployment
  or enterprise-domain configuration. Public GitHub Copilot auth may remain.
- Remote workspace-hub connect/bind/upload/recover behavior. Local filesystem,
  worktree, workspace, and independently useful local workspace-state behavior
  remain in scope.
- Remote MCP OAuth, browser consent, remote token exchange, or remote control
  plane behavior. Local MCP stdio and local/configured streamable HTTP remain.
- Remote plugin marketplace/catalog/index/install. Local descriptor-backed
  plugin/extension lifecycle remains.
- Hosted share/upload, hosted session URLs, SuperGrok/billing/paywall flows,
  hosted announcements, or product telemetry/analytics network calls.
- Hosted image/video generation commands such as `/imagine` and
  `/imagine-video`. Local image attachments and inline media rendering remain.
- Copied Grok source, tests, fixtures, snapshots, identifiers, theme tables, or
  evidence. Reference source inspection is for behavioral understanding only;
  Harness implementation must be independently authored.
- Fake success, diagnostic-only success, “Unavailable” presented as completion,
  registry-only completion, mock-only completion, stale/copied evidence,
  post-hoc provenance, self-comparison, status inflation, or acceptance from a
  binary other than the recorded candidate.

### 1.3 Branding divergence

Only product identity may differ: Harness name, logo artwork, version, and
accurate provider/account wording. Identity substitution must preserve the
reference bounding box, row count, alignment, spacing, focus behavior, and
choreography. No other visual divergence is approved.

### 1.4 Removal compatibility matrix

Removal means removal from all new public behavior, not corruption of existing
local data. Tasks 8-12 and 18 must maintain this matrix and add one fixture per
row:

| Retired family | Persisted records/config to audit | Required retained behavior |
|---|---|---|
| voice/dictation | voice settings, actions, events, audio/model metadata | new config/action rejected; old session replay is side-effect-free retired/no-op or actionable unsupported-version |
| enterprise/OIDC | enterprise URL, OIDC state/tokens, auth events/status | public auth remains usable; retired credentials atomically removed; old sessions replay safely |
| remote workspace hub | endpoint, binding, upload/recovery status and events | local workspace/session replay remains usable; remote records are retired or actionable unsupported-version |
| remote MCP OAuth | OAuth state/token/redirect/provider fields | local MCP remains usable; old remote auth data is never sent or refreshed |
| marketplace/hosted share/media | catalog/install/share/upload/media URLs and events | local plugin/export/inline-media data remains readable; hosted actions are absent |
| telemetry/announcements | analytics IDs, remote-feed cursors, tracking events | no network call; local logs/replay remain redacted and readable |

Fixtures must prove unrelated historical events still replay and that retired
credentials are not left usable on disk. No compatibility shim may re-expose a
removed public command or network path.

## 2. Reference authority and mandatory source inspection

### 2.1 Pinned executable

Use only:

```text
path: inspirations/grok-build/target/debug/xai-grok-pager
sha256: 883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5
version: grok 0.1.220-alpha.4 (c1b5909) [stable]
reference revision: c1b5909ec707c069f1d21a93917af044e71da0d7
```

Before each capture campaign, verify executable bit, digest, version, and
reference revision. The preflight must also run
`git -C inspirations/grok-build rev-parse HEAD` and require
`c1b5909ec707c069f1d21a93917af044e71da0d7`, require a clean recursive
`git -C inspirations/grok-build status --porcelain`, and record the recursive
submodule status plus an ordered source-tree manifest of path/mode/byte SHA-256.
The reference seal binds that source revision/tree digest to the pinned binary
SHA and version; if the checkout lacks a trustworthy build/provenance binding,
the reference is externally `blocked` rather than treated as source-equivalent.
Never rebuild, replace, download, update, or mutate anything under
`inspirations/grok-build`.

The next-loop preflight must run `test -x inspirations/grok-build/target/debug/xai-grok-pager`
and verify every required source root in §2.2. If this checkout does not contain
the pinned executable or source tree, the loop records an external `blocked`
state with the missing paths and stops before any product edit; it must not
silently substitute another reference or claim parity from Harness-only tests.

### 2.2 Source inspection is mandatory before implementation

Every owner must inspect the corresponding Grok source before writing a test or
implementation. At minimum, the complete crosswalk must account for all public
behavior in these roots:

```text
inspirations/grok-build/crates/codegen/xai-grok-pager/
inspirations/grok-build/crates/codegen/xai-grok-pager-render/
inspirations/grok-build/crates/codegen/xai-grok-shell/
inspirations/grok-build/crates/codegen/xai-grok-tools/
inspirations/grok-build/crates/codegen/xai-grok-config/
inspirations/grok-build/crates/codegen/xai-grok-auth/
inspirations/grok-build/crates/codegen/xai-grok-mcp/
inspirations/grok-build/crates/codegen/xai-grok-agent/
inspirations/grok-build/crates/codegen/xai-acp-lib/
inspirations/grok-build/crates/codegen/xai-grok-workspace/
inspirations/grok-build/crates/codegen/xai-grok-update/
inspirations/grok-build/crates/codegen/xai-grok-shell-session-support/
inspirations/grok-build/crates/codegen/xai-fast-worktree/
inspirations/grok-build/crates/codegen/xai-grok-sandbox/
inspirations/grok-build/crates/codegen/xai-prompt-queue/
inspirations/grok-build/crates/codegen/xai-grok-memory/
inspirations/grok-build/crates/codegen/xai-grok-hooks/
inspirations/grok-build/crates/codegen/xai-chat-state/                  # compaction/interjection state
inspirations/grok-build/crates/codegen/xai-grok-agent/src/compaction.rs  # compaction policy
inspirations/grok-build/crates/codegen/xai-hunk-tracker/
inspirations/grok-build/crates/codegen/xai-gix-status/
inspirations/grok-build/crates/codegen/xai-codebase-graph/
inspirations/grok-build/crates/codegen/xai-system-power/
inspirations/grok-build/crates/codegen/xai-tty-utils/
inspirations/grok-build/crates/codegen/xai-grok-voice/            # removal crosswalk only
inspirations/grok-build/crates/codegen/xai-grok-plugin-marketplace/ # removal crosswalk only
inspirations/grok-build/crates/codegen/xai-grok-workspace-client/   # removal crosswalk only
inspirations/grok-build/crates/codegen/xai-grok-telemetry/          # removal crosswalk only
inspirations/grok-build/crates/codegen/xai-grok-pager/docs/user-guide/
```

Each capability row must store a `reference_source` list containing exact file
paths and symbols inspected. “Inspected Grok Build” without paths is invalid.
For executable-visible states, source inspection does not replace black-box
capture. Source and binary disagreement blocks the row until resolved.

## 3. Audited starting truth

The next loop starts from this observed state and must refresh it before edits:

- Branch: `ui-ux-experiments`; HEAD at plan preparation: `d6b1e9a8`.
- Current checkout: 42 modified tracked files with approximately 9,749
  insertions and 1,964 deletions, plus a large untracked implementation/test
  set. Preserve unrelated user work and never use destructive Git commands.
- Capability inventory: 234 rows, currently 65 `pass`, 52 `incomplete`, and
  117 `blocked`. These labels are not trusted until recomputed.
- TUI parity manifest: 41 rows, currently 37 `incomplete`, 4 `blocked`, and zero
  `pass`.
- F1 failed: dependency timing, write-set discipline, missing canonical task
  evidence, skipped integration, and premature final acceptance.
- F2 failed: false-success CLI shards, provider credential/auth-profile wiring,
  and unsafe/allocation-capable Landlock child setup.
- F4 failed: status inflation, misuse of `blocked`, stale/copied or post-processed
  provenance, and no accepted installed-binary same-revision proof.
- Real excluded code is present: Copilot Enterprise auth, generic browser OIDC,
  remote workspace hub, remote MCP OAuth, marketplace strings/actions, hosted
  share paths, and voice capability scaffolding.
- Real retained code is also present but may be unintegrated or mislabeled:
  Anthropic transport/config, persistent code graph, update pipeline, worktree
  and attribution work, sandbox work, TUI leaf modules, CLI shards, and parity
  evidence helpers.

No previous task, pass row, green test, or commit is inherited as complete. A
task may be marked complete only after its current-worktree implementation and
fresh evidence satisfy this plan.

## 4. Status and evidence contract

Every machine-readable row uses exactly one status:

```text
incomplete | blocked | pass | diverged
```

- `incomplete`: implementation or applicable evidence is missing.
- `blocked`: only an external dependency prevents execution; record exact
  dependency, command, environment name, and owner. Internal work is incomplete.
- `pass`: the compiled public surface and every applicable evidence layer pass
  on the same candidate revision.
- `diverged`: only an exact user-approved divergence ID permits this value.

Applicable evidence layers:

```text
L0 reference-source crosswalk and owner
L1 unit/state transition and semantic terminal-cell proof
L2 compiled CLI/TUI/tool operation with external postcondition
L3 PTY/input/error/cancel/restart/recovery trace
L4 settled pixel plus fixed animation-tick comparison
L5 live-provider and installed-binary agent dogfood
L6 independent review and undisclosed holdout
```

Visual rows require L0-L4 and L6. Rows classified `live-required` require L0-L3,
L5, and L6. Rows classified `auth-boundary` require config-reachable transport,
credential-source, refresh/redaction, controlled local endpoint or deterministic
mock proof, and L6; they do not acquire an unapproved live credential. Rows for
providers not accepted by the Umans live matrix remain `incomplete` or `blocked`
unless the user approves a live credential and exact provider. A row may not pass
when its public action is absent, diagnostic-only, mock-only, unavailable, or
wired to a non-owning bookkeeping module.

### 4.1 Candidate identity

Each attempt has two immutable identities:

- `product_epoch`: digest of every tracked and untracked byte that can affect the
  compiled product, Cargo manifests/lockfile, toolchain, generated source/schema
  inputs, and submodule/reference identities. It is sealed after the last product
  source mutation and before Task 34 onward.
- Task 34 must emit `task-34/product-epoch-input-set.json` before the seal. It is
  an ordered, canonical JSON list of every hashed repository-relative path with
  type (tracked, untracked, generated, manifest, toolchain, or submodule), file
  mode, byte SHA-256, and source preimage; it also records the Rust/Cargo
  toolchain identity, generator inputs, submodule revisions, explicit exclusion
  rules (`target/`, evidence, runtime sessions, and caches), and the aggregate
  digest over that exact JSON. Later tasks verify this immutable input-set digest
  rather than recomputing an unspecified file glob.
- `attestation_epoch`: digest of manifests, evidence indexes, review receipts,
  and other non-compiled status artifacts. It may be created after product epoch
  sealing, but every final receipt records both digests and the exact installed
  binary digest. Status manifests are attestations, not hidden product inputs.
  The sealed attestation input set is enumerated before hashing and excludes the
  outer `task-41/final-attestation.json` and `task-42/release-stop.json` receipts;
  Task 41 records the digest in the first excluded receipt and Task 42 verifies
  the sealed set before writing the second excluded receipt.
- `base_evidence_input_set`: immutable Task 38 digest of fresh product, test,
   visual, live, and dogfood evidence available before independent review. Task
   39 binds to this base and emits `visual-review-seal.json` containing the base
   digest plus its own review digest. Task 40 independently verifies the same
   base digest and emits `runtime-review-seal.json` containing that base digest
   and its own review digest; it does not consume Task 39’s seal, so Tasks 39/40
   remain safely parallel. Task 41 consumes both review seals plus the base seal,
   verifies their shared base/candidate/product identities, and only then binds
   all three into final attestation. Reviewers never bind to the later
   `attestation_epoch`.

No evidence from different `product_epoch` values may be combined. A candidate
binary is rebuilt whenever product bytes change. A final status update must not
silently reuse a binary from an earlier product epoch.

## 5. Secret-safe live testing

- A live credential was supplied out of band. Never write it into this plan,
  source, config, command history captured as evidence, logs, events, artifacts,
  receipts, screenshots, or review prompts.
- Prefer the existing `harness.jsonc`, which references
  `UMANS_AI_CODING_PLAN_API_KEY`. Inject the credential into the candidate parent
  process only. Evidence records the variable name, presence boolean, and
  credential-source kind; it never records a presence digest or the value.
- Before any evidence is accepted, scan source changes, evidence roots, event
  logs, stdout/stderr, screenshots metadata, and support bundles for secret-like
  values. Any finding invalidates the candidate and rotates the evidence root.
- Never persist raw provider requests/responses, authorization headers, cookies,
  tokens, hidden reasoning, or unredacted tool payloads.
- The live provider credential is present only in the Harness parent process.
  Every bash, hook, ACP, MCP, plugin, and child-agent environment is constructed
  from an allowlist that strips `UMANS_AI_CODING_PLAN_API_KEY` and all provider
  credential variables. Never record a credential digest; record only variable
  name, presence boolean, and credential-source kind. A canary child must prove
  the secret is absent from its environment, arguments, inherited file
  descriptors, stdout, stderr, events, and evidence.

## 6. Parallel execution model

- One lead owns the control worktree and integration ledger.
- Read-only Grok-source auditors may run in parallel without limit imposed by
  write sets; their outputs remain claims until the lead verifies cited paths.
- Writers use dedicated worktrees and disjoint write sets. A worker may not edit
  an aggregator, manifest, root module, workspace manifest, lockfile, shared
  evidence framework, or another worker’s files unless explicitly assigned.
- Each wave has exactly one named integrator. Workers return patches and evidence;
  only the integrator mutates shared roots and manifests.
- No downstream wave starts before the preceding integration receipt exists.
- PTY, xterm.js/Chromium, native terminal, reference binary, installed candidate,
  live provider, and shared evidence-index writers use global exclusive locks.
  Task-local evidence directories are independently reserved, so disjoint
  reviewer outputs may be written concurrently.
- Each task produces an immutable evidence directory:
  `.omo/evidence/grok-build-parity-next/<attempt-id>/task-<N>/`.
- Candidate attempts use `candidate-c<N>`; rejected repairs use `repair-r<M>`.
  Evidence is never overwritten or copied forward. Later attempts inherit only
  verified digests and regenerate applicable runtime artifacts.
- No automatic commits. Do not stage, commit, rebase, reset, clean, stash, or
  push unless the user later requests it.

### 6.1 Salvage and file-level write reservations

Task 1 freezes the starting path/hash inventory. Task 3 then materializes a
secret-scanned immutable salvage overlay for every retained dirty path: tracked
patch bytes and untracked file bytes are stored only when the scanner permits
them, keyed by their Task 1 preimage hash. A worker worktree starts from HEAD,
applies only its assigned verified overlay, and refuses a mismatched preimage.
Hash-only or secret-positive paths remain owned by the control-worktree
integrator and are never materialized into worker prompts.

Task 3 completes before any source-writing task. Task 4 is therefore not allowed
to edit dirty parity files until Task 3 has classified and snapshotted them.

The scheduler must enforce this reservation table. A path not listed is forbidden
to every worker; an integrator may edit its shared roots only after all leaf
patches are reviewed:

| Tasks | Exclusive write set | Shared roots reserved to integrator |
|---|---|---|
| 1-3 | new attempt evidence, salvage overlays, ledgers | none |
| 4 | `crates/harness-testkit/src/parity/**`, its owner tests, `scripts/test-lanes.sh`, `scripts/harness-qa-dogfood.sh`, lane owner tests, assigned evidence scripts | all product roots, manifests, Cargo roots |
| 5 | `docs/capability-inventory.v1.json`, `docs/tui-reference-parity-manifest.v1.json`, subsystem validators | no source roots |
| 6 | `scripts/parity_task_qa.py`, `scripts/check-parity-*.py`, `scripts/validate-parity-*.py`, `scripts/run-independent-review.py`, `scripts/run-resolved-review.py`, scheduler/control/evidence files only | no product roots |
| 7 | attempt ledger plus `task-7/wave-0-integration.json` only; manifests are read-only inputs | no product roots or manifests |
| 8 | voice leaf modules/settings/tests/docs rows | root modules, Cargo files, shared registries |
| 9 | auth OIDC/enterprise leaf modules/tests | `lib.rs`, auth command aggregators, config/schema/docs |
| 10 | remote workspace-hub leaf module/tests | core/TUI aggregators, manifests, docs |
| 11 | remote MCP OAuth leaf module/tests | MCP aggregators, AppState, manifests, docs |
| 12 | marketplace/share/media/telemetry leaf modules/tests | CLI/TUI aggregators, manifests, docs |
| 13 | assigned CLI leaf command files/tests | `crates/harness/src/lib.rs`, help/schema aggregators |
| 14 | provider leaf/bootstrap/provider tests | workspace manifests, public config aggregators |
| 15 | sandbox/landlock/network/shell leaf files/tests | workspace manifests and root module wiring |
| 16 | sleep/wake core and platform adapter files/tests | bootstrap/root modules/config/docs |
| 17 | all root modules, Cargo files/lockfile, schemas, docs, manifests, shared registries | exclusive integrator |
| 18-23 | their named core/provider/tools/CLI leaf modules and owner tests | root aggregators and manifests |
| 24 | all Wave 2 shared roots and final local-core snapshot | exclusive integrator |
| 25 | `crates/harness-tui/DESIGN.md`, reference crosswalk/evidence only | product source roots |
| 26 | TUI composer/startup/slash leaf modules/tests | `app.rs`, `lib.rs`, root registries |
| 27 | TUI transcript/media/render leaf modules/tests | renderer/root aggregators |
| 28 | TUI overlay/session/settings/auth leaf modules/tests | `ui_overlays.rs`, `app.rs`, manifests |
| 29 | TUI dashboard/task/queue/plan/worktree leaf modules/tests | AppState/root aggregators |
| 30 | TUI input/mode/terminal/responsive/mouse leaf modules/tests | root keybinding/runtime aggregators |
| 31 | TUI theme/notification/tip leaf modules/tests | theme/runtime root aggregators |
| 32 | TUI slash/action/setting registries and their tests | AppState/render root aggregators |
| 33 | all shared TUI aggregators, manifests, capture wiring | exclusive integrator |
| 34-41 | evidence/attestation outputs except final manifest owner | no product source |
| 42 | `task-42/release-stop.json`, `task-42/oracle-input-set.json`, `task-42/resolved-terminal-command-manifest.json`, and the isolated `task-42/qa/**` output root | no product source, status manifest, or candidate bytes |

Task 17, Task 24, and Task 33 are the only owners allowed to resolve shared
root conflicts. Tasks 34-42 are read-only with respect to product source.

Tasks 1-6 are serial bootstrap work governed by the static reservations in this
plan. Before Task 7 or any dispatched Task 8+ worker starts, Task 6 must write
`.omo/evidence/grok-build-parity-next/<attempt-id>/task-qa.json` with one
normalized file path reservation and one concrete QA invocation for every task.
The broad table above is only the maximum boundary; the normalized file list is
the scheduler input and is the authority used by F1.

### 6.2 Executable QA rule for every task

Every task evidence receipt must include `command`, `cwd`, explicit inputs,
expected exit/status, expected external postcondition, failure mutation, and
artifact paths. “Run tests,” “E2E,” “matrix,” “coverage checker,” “validator,”
or “review” without these fields is not executable QA. Task 4 owns the shared
receipt validator; Task 6 owns the dependency/write-set runner. Where a named
owner test does not yet exist, the task must first add that independently
authored owner test in its write set, run it red, then run the exact command below
green. Cross-cutting commands are:

The route table names the implementer for each task. OMO does not invoke its
orchestration-level `oracle` after every task: after the implementer claims the
entire loop complete at Task 42, OMO invokes `subagent_type=oracle` once as the
terminal read-only completion review. That oracle checks the full loop claim
against this plan and the sealed evidence; it does not implement tasks, write
task receipts, or self-review an oracle-owned task. Its verdict is recorded in
OMO orchestration metadata. A rejection reopens the earliest named owner in a
fresh repair namespace and the loop continues.

| Tasks | Required executable QA | Expected result |
|---|---|---|
| 1-3 | `test -x inspirations/grok-build/target/debug/xai-grok-pager`; start-state/salvage runner with fake-token and preimage mutations | reference preflight passes; secret materialization and mismatched overlays fail |
| 4-6 | `python3 scripts/check-test-suite-gates.py`; evidence/scheduler mutation runner created by the task | provenance, dependency, overlap, stale/copy, and secret contradictions fail closed |
| 8-12 | package owner `cargo nextest` plus explicit `rg` absence checks recorded in each task receipt | removed public surface absent; retained local surface still compiles and exercises its failure path |
| 13-16 | targeted `cargo nextest` owner tests plus compiled CLI/sandbox/power invocations named in each receipt | real authority, typed failure, child security, and native adapter contracts hold |
| 17 | `cargo check --workspace`; targeted nextest; `scripts/test-lanes.sh fast` | one integrated Wave 1 product epoch with no orphan imports or scope resurrection |
| 18-23 | package owner nextest, CLI subprocess with temp workspace, and each task’s named bad-input/restart mutation | local runtime/CLI postconditions persist and recover; no probe-only success |
| 24 | full owner nextest plus separate `scripts/test-lanes.sh fast`, `scripts/test-lanes.sh quality-gates`, and `scripts/test-lanes.sh integration` invocations | local-core-green snapshot seals one product epoch |
| 25-32 | `cargo nextest run -p harness-tui`; PTY owner test with scripted input; semantic-cell/pixel comparator | retained TUI action/state passes; one-cell/one-input mutation fails |
| 33-34 | `HARNESS_BIN="$HARNESS_TUI_DRAFT_BIN" RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 scripts/test-lanes.sh signoff-pty`; manifest mutation runner; product-epoch seal verifier | Task 33 sets `HARNESS_TUI_DRAFT_BIN` to the absolute draft path; draft binary path/SHA/version is verified, fresh candidate draft exists, and no status promotion occurs |
| 35-37 | explicit installed `HARNESS_BIN` smoke/live/PTY commands with path/SHA/version checks | all artifacts come from the sealed installed candidate; missing/alternate binary fails |
| 38 | every command in Task 38’s literal shell block, with no skipped stages | all gates exit zero and write fresh receipts |
| 39-40 | independent reviewer-agent invocation plus fresh deterministic/PTY rerun and undisclosed mutation/holdout inputs | reviewer returns unconditional approval or typed blocking findings |
| 41-42 | F4 twice, stale/copy/self-comparison mutation, final secret scan, read-only consistency check | final attestation passes; deliberate mutations fail; exact stop eligibility is proven |

The task receipt validator rejects a receipt missing any required field. The
executor must expand grouped rows into one receipt per task; grouped wording is
only a command-family shorthand, never permission to omit a task’s input or
expected result.

Task 6’s QA-dispatch map must contain these exact task keys and expected outcomes
(the runner may add flags, but may not omit a key):

| Task | Required runner mode | Expected result |
|---:|---|---|
| 1 | `--task 1 --start-state --secret-mutation` | reference preflight and hash-only snapshot pass; fake secret materialization fails |
| 2 | `--task 2 --reference-crosswalk` | every required source/command/action/view has one cited row |
| 3 | `--task 3 --salvage-overlay --preimage-mutation` | Task 1 path set is covered; mismatched overlay is refused |
| 4 | `--task 4 --provenance-mutations` | stale/copy/post-process/secret contradictions fail |
| 5 | `--task 5 --status-mutations` | pass-with-residual/internal-blocked/removed-surface rows fail |
| 6 | `--task 6 --scheduler-mutations` | dependency and reservation overlap mutations fail |
| 7 | `--task 7 --wave-0` | clean validator/owner snapshot passes |
| 8 | `--task 8 --absence voice` | no voice/STT public or dependency surface remains |
| 9 | `--task 9 --absence enterprise-oidc --replay retired-auth` | public auth works; retired auth is absent and historical fixtures are safe |
| 10 | `--task 10 --absence remote-workspace --local-workspace` | remote hub absent; local workspace journey passes |
| 11 | `--task 11 --mcp local-loopback local-configured-nonloopback --absence oauth` | retained transports pass; OAuth/discovery/redirect mutations fail |
| 12 | `--task 12 --absence hosted-marketplace-share-media-telemetry` | excluded surfaces/network calls are absent; local export/media pass |
| 13 | `--task 13 --cli-authority-mutations` | every retained command has real postcondition and failure path |
| 14 | `--task 14 --provider-auth-matrix` | credential source/refresh/redaction and provider routing are truthful |
| 15 | `--task 15 --sandbox-child-mutations` | READY/EOF/fd/network/security mutations fail closed |
| 16 | `--task 16 --power-supervisor` | singleton/adapters/shutdown/refresh race contract passes |
| 17 | `--task 17 --wave-1-integration` | shared roots compile and all accepted patches apply once |
| 18 | `--task 18 --session-recovery-matrix` | replay/rewind/restart/retired-data fixtures pass |
| 19 | `--task 19 --memory-queue-compaction` | persistence/version/drain/flush/compaction mutations pass |
| 20 | `--task 20 --workspace-vcs-trust` | isolation/path/trust/attribution/cleanup journeys pass |
| 21 | `--task 21 --orchestration-matrix` | task/team/cron/wait/cancel/restart journeys pass |
| 22 | `--task 22 --local-integrations-matrix` | hooks/plugins/ACP/MCP/graph/update/export boundaries pass |
| 23 | `--task 23 --cli-config-matrix` | help/JSON/errors/settings/provider/config journeys pass |
| 24 | `--task 24 --wave-2-integration` | local-core-green seals one product epoch |
| 25 | `--task 25 --reference-freeze --identity-mutations` | only approved identity spans differ; all other mutations fail |
| 26 | `--task 26 --tui-shell-composer-pty` | startup/draft/input/completion/mode journeys match |
| 27 | `--task 27 --tui-transcript-media-pty` | blocks/streaming/diff/media/selection journeys match |
| 28 | `--task 28 --tui-overlay-matrix` | every overlay entry/exit/error/persist path matches |
| 29 | `--task 29 --tui-dashboard-journeys` | multi-agent/dashboard/task/queue/worktree journeys match |
| 30 | `--task 30 --tui-mode-terminal-matrix` | vim/minimal/fullscreen/input/resize/fallback paths match |
| 31 | `--task 31 --tui-theme-notice-matrix` | theme/notification/tip/system preference paths match |
| 32 | `--task 32 --tui-registry-mutations` | retained registry coverage passes; removed/no-op/duplicate actions fail |
| 33 | `--task 33 --wave-3-tui-signoff` | all TUI evidence is fresh and same-candidate |
| 34 | `--task 34 --inventory-draft --product-epoch-seal` | product inputs seal; no status promotion occurs |
| 35 | `--task 35 --candidate-install --umans-unique-model-matrix --alias-resolution` | install seal and the three unique model journeys pass; Coder/Flash aliases resolve to their covered models; child secret canary fails closed |
| 36 | `--task 36 --installed-dogfood` | real agent journeys produce external postconditions |
| 37 | `--task 37 --installed-pty-native` | exact `HARNESS_BIN` path/SHA/version is used for every artifact |
| 38 | `--task 38 --full-gates` | every literal gate exits zero with fresh receipt |
| 39 | `--task 39 --independent-visual-review` | holdouts and reviewer return unconditional approval |
| 40 | `--task 40 --independent-runtime-review --f1-f4` | F1-F4 and security/rejection schema pass |
| 41 | `--task 41 --final-attestation --f4-mutation` | final statuses promote only after all evidence; stale mutation fails |
| 42 | `--task 42 --read-only-release-stop --oracle-input-set` | isolated final commands, secret scan, consistency, and the sealed terminal-oracle input set all pass |

### 6.3 Maximum-safe parallelism and agent routing

The scheduler is dynamic: after the static dependencies and file reservations are
satisfied, it dispatches every ready task whose write set is disjoint from every
other ready task. The listed groups are the maximum safe frontier for this plan;
the lead must not serialize them merely for convenience. A global lock still
serializes reference captures, live-provider calls, installed-binary materialization,
and evidence-root writes when those resources are shared.

| Tasks | Route | Required skills | Parallelism rule |
|---|---|---|---|
| 1 | `deep` | `karpathy-guidelines`, `programming` | serial bootstrap; record `HARNESS_PARITY_PLAN` as the absolute canonical root plan path |
| 2 | `explore` then `writing` | `karpathy-guidelines` | source auditors parallel internally; artifact writer after Task 1 |
| 3 | `deep` | `karpathy-guidelines`, `programming` | serial after Task 2 |
| 4 | `ultrabrain` | `karpathy-guidelines`, `programming`, `rust-best-practices` | serial before any product writer |
| 5 | `deep` | `karpathy-guidelines`, `programming` | serial after Task 4 |
| 6 | `ultrabrain` | `karpathy-guidelines`, `programming`, `rust-best-practices` | serial; creates the scheduler and QA runners |
| 7 | `deep` integrator | `karpathy-guidelines`, `programming` | exclusive gate |
| 8-13 | `deep` | `karpathy-guidelines`, `programming`, `rust-best-practices` | Tasks 8/9/10/11/12/15 run together after Task 7; Task 13 starts after its named dependencies |
| 14-16 | `ultrabrain` | `karpathy-guidelines`, `programming`, `rust-best-practices`, `rust-async-patterns` | Task 14 runs after Task 9; Task 16 runs after Task 14; Task 15 runs with 8/9/10/11/12 |
| 17 | `deep` integrator | `karpathy-guidelines`, `programming`, `rust-best-practices` | exclusive Wave 1 gate |
| 18 | `deep` | `karpathy-guidelines`, `programming`, `rust-best-practices`, `rust-async-patterns` | runs parallel with Tasks 19/20/21/22 |
| 19 | `ultrabrain` | `karpathy-guidelines`, `programming`, `rust-best-practices`, `rust-async-patterns` | runs parallel with Tasks 18/20/21/22 |
| 20-23 | `deep` | `karpathy-guidelines`, `programming`, `rust-best-practices` | Tasks 20/21/22 run with 18/19; Task 23 starts after 13-22 |
| 24 | `deep` integrator | `karpathy-guidelines`, `programming`, `rust-best-practices` | exclusive Wave 2 gate |
| 25 | `visual-engineering` | `karpathy-guidelines`, `programming`, `rust-best-practices`, `shared/frontend`, `shared/visual-qa` | exclusive reference-capture lock; source audit may fan out |
| 26-27 | `visual-engineering` | `karpathy-guidelines`, `programming`, `rust-best-practices`, `shared/frontend`, `shared/visual-qa` | run together after Task 25 |
| 28-30 | `visual-engineering` | `karpathy-guidelines`, `programming`, `rust-best-practices`, `shared/frontend`, `shared/visual-qa` | Task 28 after 26/27; Task 29 after 20-22 and 25-28; Task 30 after Task 29 |
| 31 | `visual-engineering` | `karpathy-guidelines`, `programming`, `rust-best-practices`, `shared/frontend`, `shared/visual-qa` | runs in parallel with Tasks 26-30 after Task 25 because its leaf write set is disjoint |
| 32 | `visual-engineering` | `karpathy-guidelines`, `programming`, `rust-best-practices`, `shared/frontend`, `shared/visual-qa` | starts after Tasks 26-31 |
| 33 | `visual-engineering` integrator | `karpathy-guidelines`, `programming`, `rust-best-practices`, `shared/frontend`, `shared/visual-qa` | exclusive TUI gate |
| 34 | `deep` | `karpathy-guidelines`, `programming` | starts after Tasks 24/33; read-only product source |
| 35 | `deep` | `karpathy-guidelines`, `programming`, `rust-best-practices`, `rust-async-patterns` | starts after Tasks 14/23/24/34; exclusive live-provider lock |
| 36 | `deep` | `karpathy-guidelines`, `programming`, `rust-best-practices` | starts after Task 35 |
| 37 | `visual-engineering` | `karpathy-guidelines`, `programming`, `rust-best-practices`, `shared/frontend`, `shared/visual-qa` | starts after Task 36; exclusive PTY/native lock |
| 38 | `unspecified-high` | `karpathy-guidelines`, `programming`, `rust-best-practices` | exclusive full-gate runner |
| 39 | `visual-engineering` | `shared/frontend`, `shared/visual-qa` | implementer runs the independent visual review; runs in parallel with Task 40 after Task 38 |
| 40 | `unspecified-high` | `karpathy-guidelines`, `programming`, `rust-best-practices`, `rust-async-patterns` | implementer runs the independent runtime review after Task 38; OMO reviews the complete loop only after Task 42 |
| 41 | `unspecified-high` integrator | `karpathy-guidelines`, `programming` | exclusive final status promotion after Tasks 39/40 |
| 42 | `unspecified-high` | `karpathy-guidelines`, `programming`, `rust-best-practices` | implementer writes the final read-only consistency receipt; OMO reviews the complete loop only after Task 42 |

For `explore` and `writing` use the corresponding direct `subagent_type`; for
all other rows use the named configured `category`. The terminal `oracle` is not
an implementer route or a project profile under `.agent-harness/agents/`; OMO
invokes it once after the Task 42 implementer claims the entire loop complete.
It is read-only and judges the full sealed result. Every delegated
prompt must include the task number, exact write set, dependencies, GOAL, STOP
WHEN, EVIDENCE, and the required QA-dispatch mode from §6.2. A worker that finds
its route undersized may ask the lead to split the task, but may not silently
expand its write set.

### 6.4 Implementer freedom within hard guardrails

The plan constrains observable outcomes and safety, not internal implementation
style. The implementer may choose:

- module boundaries and private helper structure inside its reserved paths;
- concrete Rust types, state representation, ownership/borrowing strategy,
  sync versus async structure, and error-enum shape;
- the smallest independently authored test fixtures and the exact internal
  coordinator seam used to reach a required public action;
- whether an existing Harness helper is reused or a focused new helper is added;
- the visual component decomposition, provided measured geometry and interaction
  behavior remain exact;
  - an existing dependency, or a proposed in-scope new dependency when it is
    justified in the task receipt with license, version, build, security, and
    offline behavior. Workers may propose dependency changes but may not edit
    Cargo manifests or `Cargo.lock`; only the owning integrator (Tasks 17, 24,
    or 33) may apply and validate them after reviewing the proposal.

The implementer may not change the locked product scope, reference identity,
branding allowance, event authority, replay purity, permission ordering,
redaction, cancellation, persisted-data compatibility, candidate/evidence
identity, write reservations, or final stop criteria. It may not copy Grok code,
tests, fixtures, identifiers, themes, or evidence. It may not add an external
service, network path, public command, config key, or compatibility alias merely
to make a test pass.

When reference behavior is ambiguous, the implementer uses the cited source and
pinned executable, chooses the simplest Harness-native design that preserves the
observable contract, and records the decision and evidence in the task receipt.
Only a product-scope divergence or destructive persisted-data decision that is
not covered by §1.4 escalates to the lead/user; internal design choices do not
require an interview. Reviewers judge the result against the contract, not a
preferred internal architecture.

## 7. Execution checklist

### Wave 0: preserve work, establish truth, and repair evidence foundations

- [ ] **Task 1 — Freeze the exact starting state without leaking secrets**
  - **Depends:** none.
  - **Write set:** new attempt evidence root only.
  - Record HEAD, branch, submodules, Rust/tool versions, worktree status, tracked
    diff hashes, untracked path/hash inventory, reference executable identity,
    config path hashes, manifest hashes, and environment variable names.
  - Sensitive or scanner-positive files are metadata/hash-only and are never
    archived. Do not package `harness.jsonc`, credential stores, sessions, or
    provider payloads.
  - **QA:** mutate a copied fixture with a fake token and prove the snapshot
    scanner rejects materialization while retaining path/hash metadata.
  - **Evidence:** `task-1/start-state.json`, `task-1/secret-scan.json`.

- [ ] **Task 2 — Build the complete Grok source crosswalk**
  - **Depends:** Task 1.
  - **Write set:** crosswalk artifacts and draft inventory only.
  - Account for every public Grok slash module, action ID, screen mode, view,
    setting, CLI command/flag, runtime subsystem, integration, and documented
    journey in the roots from §2.2. Grouping is allowed only when every member is
    enumerated.
  - Mark each item `keep`, `remove`, `reference-only`, or `identity-divergence`.
  - Record exact Grok path/symbol, executable probe, Harness owner, and current
    state. No recommendation may rely only on filenames or existing Harness
    manifests.
  - **QA:** coverage checker fails when any reference registry member or public
    command is omitted or duplicated.
  - **Evidence:** `task-2/reference-crosswalk.json` and coverage receipt.

- [ ] **Task 3 — Inventory and classify every dirty/untracked Harness path**
  - **Depends:** Tasks 1-2.
  - **Write set:** salvage ledger only.
  - For every current modified or untracked path, record hash, subsystem,
    originating wave when recoverable, owning crosswalk rows, `retain`, `rework`,
    `remove`, or `unrelated`, and the future integration task.
  - Never discard a path solely because prior evidence failed. Never preserve a
    path solely because it compiles.
  - Materialize the immutable, secret-scanned salvage overlay described in §6.1.
    For every materialized path record the Task 1 preimage hash, overlay hash,
    scanner result, owner, and allowed worker worktree. The live checkout is not
    the comparison baseline after this task.
  - **QA:** compare the ledger path set to the Task 1 `start-state.json` path set,
    not live `git status`; mutate a preimage before overlay application and prove
    the worker refuses it.
  - **Evidence:** `task-3/salvage-index.json`, `task-3/salvage-overlay-index.json`.

- [ ] **Task 4 — Repair immutable evidence provenance and runner ownership**
  - **Depends:** Tasks 1-3.
  - **Write set:** `crates/harness-testkit/src/parity/**`, parity owner tests,
    evidence scripts assigned exclusively to this task.
  - Remove every copy/reuse/post-process path. Capture source/binary/reference/
    manifest/scenario/environment identity at artifact creation time.
  - Enforce canonical task directories, runner hash/version, command metadata,
    fresh-root creation, same-candidate identity, and fail-closed secret scans.
  - Fix `ArtifactReceipt` and status validation so contradictory secret-scan,
    provenance, owner, or evidence fields fail.
  - Repair `scripts/test-lanes.sh` so every acceptance mode (fast, integration,
     quality-gates, simulation, binary, PTY, live, native, and all-deterministic)
     propagates non-zero stage status; no acceptance stage may use `|| true` or
     convert a failed prerequisite into `SKIP`. Repair signoff-binary/signoff-pty
     and `scripts/harness-qa-dogfood.sh` so they require an explicit executable
     `HARNESS_BIN`, record its path/permissions/SHA/version, invoke that exact
     binary (never `cargo run` or an alternate `target/debug/harness`), and
     propagate non-zero status. The lane entrypoint must exit non-zero whenever
     any stage fails or any required prerequisite is missing; only explicit
     dry-runs may return success without executing stages. Keep dry-run behavior
     explicit and non-accepting.
     Update the lane/dogfood owner tests and `docs/testing.md` with these strict
     semantics.
  - **QA:** run the owner lane/dogfood tests plus injected failing-stage and
     alternate-binary mutations. The suite must reject stale root, copied file,
     changed runner, wrong revision, missing task evidence, post-hoc provenance,
     self-comparison, clean=true with non-empty findings, `cargo run` dogfood,
     an ignored `HARNESS_BIN`, and any lane that reports PASS after a non-zero
     stage.
  - **Evidence:** `task-4/evidence-framework-mutations.json`.

- [ ] **Task 5 — Reset manifests to truthful current status**
  - **Depends:** Tasks 2 and 4.
  - **Write set:** capability/TUI/subsystem manifests and validators only.
  - Recompute every row from current compiled public behavior and current fresh
    evidence. Internal gaps become `incomplete`, not `blocked`. Removed features
    leave the required product inventories and move to a scope-removal ledger;
    they may not linger as planned implementation.
  - All current TUI rows remain incomplete until fresh L0-L4 evidence exists.
  - **QA:** tamper tests reject pass-with-residual-notes, nonexistent owners,
    internal blockers, missing evidence, removed-surface resurrection, and stale
    revision hashes.
  - **Evidence:** `task-5/status-recompute.json`.

- [ ] **Task 6 — Repair the scheduler, write-set, and integration ledger**
  - **Depends:** Tasks 1-5.
  - **Write set:** next-loop control/evidence files only.
  - Define dependency-complete dispatch, reserved paths, worker/integrator roles,
    start/completion timestamps, patch hashes, one-application-only integration,
    rejected-patch reasons, and canonical evidence paths.
  - Create the task QA runner, F1-F4 validators, and independent-review runner
    named by §6.2 and §7.1. The review runner launches a fresh read-only process,
    records reviewer identity/tool/model/version, binds reference/product/
    candidate/evidence digests, validates typed findings or signed approval, and
    fails closed on missing provenance.
  - Scheduler must prevent a task starting before all dependencies complete and
    prevent overlapping writer reservations.
  - **QA:** synthetic dependency and overlapping-write mutations must be refused.
  - **Evidence:** `task-6/scheduler-mutation-receipt.json`.

- [ ] **Task 7 — Wave 0 integration gate**
  - **Depends:** Tasks 3-6.
  - **Write set:** attempt ledger and Task 7 evidence receipt only; root manifests
    are read-only inputs already owned by Task 5.
  - Verify no source behavior was accepted from old status claims; salvage index
    covers the full dirty tree; evidence mutations pass; and every following task
    has disjoint write ownership.
  - **QA:** run manifest validators and parity evidence unit tests from a clean
    temporary CARGO_TARGET_DIR.
  - **Evidence:** `task-7/wave-0-integration.json`.

### Wave 1: remove excluded product surfaces and fix high-severity foundations

- [ ] **Task 8 — Remove voice/dictation completely**
  - **Depends:** Task 7.
  - Remove voice capability rows, leaf actions/views, `/voice`, keybindings,
    settings, status fields, tests that assert a voice owner, dependencies, docs,
    and planned evidence. Do not remove inline image attachments/media rendering.
  - **QA:** source/config/schema/help/palette/TUI absence tests; dependency tree
    contains no STT/Whisper/voice crate; old config keys fail clearly.
  - **Evidence:** `task-8/voice-absence.json`.

- [ ] **Task 9 — Remove Enterprise SSO, generic browser OIDC, and Copilot Enterprise**
  - **Depends:** Task 7.
  - Remove `browser_oidc`, `browser_oidc_local`, generic OIDC state/status/tests,
    enterprise SSO inventory rows, `CopilotDeployment::Enterprise`, enterprise
    URL prompts/config/persistence, and TUI diagnostics.
  - Preserve public Copilot device auth, Codex OAuth/device/browser-loopback auth,
    API-key auth, credential storage, and refresh.
  - Add a persisted-state migration contract: old enterprise/OIDC events, config
    keys, credential files, and status fields must replay side-effect free as
    typed retired/unsupported records or be rejected with an actionable version
    error. Atomically remove credentials used only by the retired flows; never
    break unrelated historical session replay.
  - **QA:** public Copilot and Codex happy/error tests pass; enterprise/OIDC keys,
    commands, fields, events, and UI are absent and rejected; fixtures containing
    retired persisted data pass the compatibility/rejection contract.
  - **Evidence:** `task-9/auth-scope.json`.

- [ ] **Task 10 — Remove remote workspace hub while preserving local workspace behavior**
  - **Depends:** Task 7.
  - Remove remote connect/bind/upload/recover code, curl/network paths, capability
    rows, AppState/status-dialog fields, probes, tests, docs, and config.
  - Preserve workspace roots, path safety, worktrees, folder trust, and local
    file-backed state only when it has an independently useful local workflow.
  - **QA:** no remote hub endpoint or network call remains; local workspace and
    worktree journeys pass.
  - **Evidence:** `task-10/workspace-scope.json`.

- [ ] **Task 11 — Remove remote MCP OAuth; preserve local MCP transports**
  - **Depends:** Task 7.
  - Remove remote OAuth availability, PKCE/browser consent/token exchange,
    credentials tied only to remote OAuth, TUI diagnostics, and capability rows.
  - Preserve config-backed stdio and streamable HTTP MCP for both loopback and
    explicitly user-configured non-loopback endpoints. Keep generic connection
    status, liveness, reconnect, tool/resource/prompt registration, permission
    gates, static/configured credential headers, and redacted diagnostics. Remove
    only OAuth discovery, PKCE, browser consent, hosted provisioning, and token
    refresh/control-plane behavior.
  - “Configured non-loopback” means a complete endpoint is present in the merged
    Harness config; there is no discovery, provisioning, implicit endpoint
    expansion, redirect following to another endpoint, or OAuth fallback. Static
    configured credentials stay inside the transport boundary and are stripped
    from child environments/evidence.
  - **QA:** loopback-server and configured non-loopback contract E2E success/error/
    restart tests; unconfigured endpoint, redirect, OAuth, and redaction fixtures
    fail closed; no remote provisioning surface remains.
  - **Evidence:** `task-11/mcp-scope.json`.

- [ ] **Task 12 — Remove marketplace, hosted share/upload, hosted media generation, and telemetry**
  - **Depends:** Task 7.
  - Remove marketplace/catalog/index/remote-install UI and CLI, hosted share URLs
    or upload commands, `/imagine`, `/imagine-video`, billing/paywall/SuperGrok,
    hosted announcements, and telemetry/analytics network behavior.
  - Preserve local descriptor plugin lifecycle, local hooks/skills/extensions,
    local transcript export, local support trace bundles, local update, and inline
    image/media presentation.
  - **QA:** help/palette/config/schema absence tests plus a network-deny integration
    test proving retained offline startup emits no analytics/hosted calls.
  - **Evidence:** `task-12/hosted-scope.json`.

- [ ] **Task 13 — Replace false-success CLI shards with real authority or remove them**
  - **Depends:** Tasks 7, 12, and 15 for the CLI leaf files and sandbox/restriction-
    owned commands. It does not wait for unrelated removal leaf tasks 8-11.
  - Audit every new CLI command/flag against Grok source and the Harness owner.
    Commands retained by the crosswalk must call the real coordinator/session/
    permission/config/provider/integration authority and expose meaningful bad
    input/failure behavior. Commands that exist only to match help text are
    removed.
  - Includes dashboard, check, restrictions, session flags, agent/model flags,
    screen modes, plan, memory, setup, MCP, ACP, and health/readiness shards.
  - **QA:** one happy and one failure E2E per advertised command with external
    postconditions; mutation replacing owner call with constant success fails.
  - **Evidence:** `task-13/cli-authority-matrix.json`.

- [ ] **Task 14 — Restore provider credential/auth-profile construction**
  - **Depends:** Task 9.
  - Trace `bootstrap.rs -> config/provider.rs -> harness-providers` and ensure
    supported providers receive resolved credential source, auth profile,
    refresh behavior, endpoint/options, and redacted metadata.
  - Reconcile Anthropic support truthfully: either the current Anthropic
    transport is fully config-reachable and live-testable, or its supported claim
    is demoted. Do not add non-Umans live acceptance requirements.
  - **QA:** stored/env/inline/missing/expired credentials; refresh single-flight;
    provider construction; redaction; malformed/error response.
  - **Evidence:** `task-14/provider-auth.json`.

- [ ] **Task 15 — Make Linux shell sandbox child setup safe and complete**
  - **Depends:** Task 7.
  - Remove allocation, locks, formatting, heap work, and non-async-signal-safe
    operations from `pre_exec`; preserve typed parent-side setup errors through a
    dedicated CLOEXEC error pipe and READY/ERROR/EOF protocol. Use a trusted
    post-exec single-threaded helper for allocating setup, not an allocating
    `pre_exec` call.
  - Define the child contract explicitly: parent-owned non-socket fd0/1/2 or a
    fail-closed socket policy, `close_range`/explicit FD allowlist, `no_new_privs`,
    Landlock for filesystem and supported TCP rights, and seccomp or equivalent
    denial for socket creation/operations, UDP/Unix sockets, io_uring setup,
    pidfd fd acquisition, descriptor passing, and bypasses. The helper emits
    `READY` only after fd closure and all required restrictions are installed;
    the parent rejects EOF/exit-before-READY and unsupported required facilities.
    A requested network-deny profile fails closed when required kernel facilities
    are unavailable.
  - Do not expand acceptance to macOS/Windows sandbox parity in this loop.
  - **QA:** static call-target/allocator gate, focused sandbox integration,
    denied-network mutation, and child setup failure propagation.
  - **Evidence:** `task-15/landlock-safety.json`.

- [ ] **Task 16 — Implement native OS sleep/wake credential protection**
  - **Depends:** Task 14.
  - Use `inspirations/grok-build/.../xai-system-power/` as the behavior source.
    Implement real platform sources: Linux logind D-Bus `PrepareForSleep` with
    delay inhibitor, macOS system-power notifications, and Windows suspend/resume
    notifications. Unsupported environments fail honestly without disabling the
    injectable test source.
  - Assign one process-scoped supervisor from runtime bootstrap. It owns singleton
    registration, reconnect/backoff, inhibitor acquisition/release, event fan-out,
    credential-manager integration, and clean shutdown. Per-session monitors are
    forbidden. Separate product adapter proof from host-native event proof; only
    the latter may be externally blocked with exact platform/service diagnostics.
  - Prevent starting one-time/rotating credential refresh during sleep or unsafe
    dark-wake conditions; allow in-flight refresh to complete within platform
    budget; resume/wake triggers near-expiry refresh through the real credential
    manager. Persist no secrets in events.
  - **QA:** platform-adapter unit tests, controlled Linux integration when
    available, injected sleep/wake race tests, cancel/shutdown, missing service,
    dark-wake/unknown state, and refresh redaction.
  - **Evidence:** `task-16/system-power.json`.

- [ ] **Task 17 — Wave 1 shared-root integration**
  - **Depends:** Tasks 8-16.
  - Apply each accepted patch once, resolve root modules/manifests/config/schema/
    docs, remove orphan tests/imports, and re-run absence plus retained-owner
    suites. The salvage ledger must mark every touched dirty path integrated,
    superseded, removed, or unrelated.
  - **QA:** from a fresh integration worktree, run `cargo check --workspace`,
    `cargo nextest run -p harness-core -p harness-providers -p harness-tools -p harness`,
    the retained/excluded help and config absence checks, and the salvage-overlay
    preimage verifier. Expected result: one applied patch per accepted worker,
    no orphan imports/tests, no scope-excluded public surface, and every changed
    path assigned in the salvage ledger.
  - **Evidence:** `task-17/wave-1-integration.json`.

### Wave 2: complete retained local runtime and CLI behavior

- [ ] **Task 18 — Sessions, resume/fork/clone/tree/rename/rewind/recovery/import**
  - **Depends:** Task 17.
  - Match retained Grok public behavior for session picker/list, stable-prefix
    lineage, rename, replay purity, atomic rewind, crash scan/repair, restart,
    cleanup, and local foreign-session discovery/import where retained.
  - Execute every fixture from the §1.4 removal compatibility matrix and prove
    retired records never trigger removed network/auth/media behavior while
    unrelated historical events replay normally.
  - No replay path may execute provider/tool/hook/MCP/network/CLI work.
  - **QA:** compiled CLI and TUI journeys for happy, corrupt, locked, invalid
    cutoff, cancellation, restart, and different-process-CWD replay.
  - **Evidence:** `task-18/sessions.json`.

- [ ] **Task 19 — Memory, prompt queue, interjection, and compaction integration**
  - **Depends:** Task 17.
  - Complete durable scoped memory, search, settings, TUI access, queued prompt
    persistence/ordering/editing, safe interjection drains, automatic queue drain,
    compaction checkpoints/suppression, and pre-compaction memory flush where the
    retained crosswalk requires it.
  - **QA:** restart persistence, stale-version edit, concurrent drain, secret
    redaction, compact/replay, and malformed store tests plus CLI/TUI E2E.
  - **Evidence:** `task-19/memory-queue-compaction.json`.

- [ ] **Task 20 — Worktrees, VCS, attribution, path safety, and folder trust**
  - **Depends:** Tasks 15 and 17.
  - Complete worktree create/select/switch/remove/cleanup, collision rollback,
    COW fast path with safe fallback, VCS status/diff, agent/external edit
    attribution, diff/blame UX, durable folder trust, and TUI trust prompt.
  - **QA:** isolated real-git journeys, deny-before-spawn, trust persistence,
    path traversal, concurrent worktrees, attribution drift/revert, and cleanup.
  - **Evidence:** `task-20/workspace-vcs.json`.

- [ ] **Task 21 — Tasks, background output, teams, cron, and agent orchestration**
  - **Depends:** Task 17.
  - Complete foreground/background transitions, wait-any/all, cancellation, late
    results, notifications, category/subagent routing, team mailbox operations,
    cron fire/dedup/restart, and real TUI task/queue/dashboard projections.
  - Grok-absent Harness-native features remain supported but must fit Grok shell
    interaction patterns and never invent conflicting primary chrome.
  - **QA:** concurrency, cancellation, restart, permission denial, duplicate
    delivery, scheduler edge cases, and real TUI/CLI journeys.
  - **Evidence:** `task-21/orchestration.json`.

- [ ] **Task 22 — Local hooks, plugins/extensions, ACP, MCP, code graph, LSP, update, export/trace**
  - **Depends:** Tasks 11-13 and 17.
  - Complete all retained local integration lifecycles and public UI/CLI actions.
    ACP stdio must have a real server surface; plugins remain local descriptor/
    lifecycle based; MCP is local/configured transport only; code graph supports
    definitions/callers/callees/references; update performs check/download/hash/
    apply/rollback/restart; export/trace stay replay-derived and redacted.
  - **QA:** one real boundary E2E plus bad input, permission denial, process
    failure, cancellation/restart, and redaction for each integration family.
  - **Evidence:** `task-22/integrations.json`.

- [ ] **Task 23 — Config, settings, provider/model selection, doctor, and operator CLI parity**
  - **Depends:** Tasks 13-16 and 18-22.
  - Complete retained Grok-visible flags, aliases, help, text/JSON/streaming JSON,
    exit codes, settings metadata/persistence, themes/modes config, migrations,
    provider/model selection, doctor readiness, completions, check loop, and
    best-of-N when retained by Task 2.
  - Help text never advertises an unavailable product action.
  - Add or verify the typed model-alias resolver contract from §1.1 and test both
    alias mappings without making network calls.
  - **QA:** CLI contract matrix with happy/error/unknown/conflict/JSON schemas and
    persisted effects; config redaction and source attribution.
  - **Evidence:** `task-23/cli-config.json`.

- [ ] **Task 24 — Wave 2 core/config/CLI integration**
  - **Depends:** Tasks 18-23.
  - Integrate shared core/config/event/CLI roots serially. Run owner suites,
    cross-crate integration, replay purity, provider auth, sandbox, and absence
    tests. Emit one immutable local-core-green snapshot for TUI workers.
  - **QA:** from a new integration worktree with the verified salvage overlays,
    run `cargo fmt --all -- --check`, `cargo check --workspace`,
    `cargo nextest run --workspace`, `scripts/test-lanes.sh fast`, and all
    retained local-core owner tests. Then run eight offline owner journeys:
    inspect/edit/verify a file; deny a mutation and recover; cancel and resume;
    create/use/clean a worktree; resume/fork/rewind a session; call local MCP or
    ACP; change and attribute a setting; and recover from a provider error.
    Expected result: every retained CLI/core action has a real
    postcondition, replay remains side-effect free, and `local-core-green` seals
    one product epoch for TUI work.
  - **Evidence:** `task-24/wave-2-integration.json` and `task-24/local-core-green.json`.

### Wave 3: achieve exact retained Grok Build TUI/UX parity

- [ ] **Task 25 — Finish reference measurement and the TUI design contract**
  - **Depends:** Tasks 2, 4, and 24.
  - Inspect and capture every retained Grok view/action/state from the pager,
    renderer, themes, actions, slash registry, settings, views, and user guide.
  - Replace every TBD in `crates/harness-tui/DESIGN.md` with measured geometry,
    colors, glyph roles, focus/cursor rules, z-order, animation ticks, and
    responsive behavior. Removed features are documented only as absence rules.
  - Capture at `120x50`, `120x40`, `120x32`, `100x30`, `80x24`, `79x24`,
    `60x20`, and one width above 120, plus reference-observed extremes.
  - Define a paired identity comparator: Harness branding substitutions are
    compared by independently declared cell spans and approved text/art bounds;
    every non-identity cell, geometry, spacing, color, focus, cursor, and timing
    value remains exact. Pin executable, terminal/parser, renderer, Chromium or
    xterm.js, font bytes, locale, Unicode width, color mode, viewport, DPR, and
    fixed clock/ticks in the capture receipt.
  - **QA:** run the pinned reference three times and the Harness fixture three
    times at every required viewport; mutate one border cell, one spacing cell,
    one color, one cursor position, and one non-identity glyph and prove the
    comparator rejects each. Identity-only substitutions must pass only when the
    declared bounds and geometry remain exact.
  - **Evidence:** `task-25/reference-freeze/` and design-contract receipt.

- [ ] **Task 26 — Startup, welcome, shell chrome, composer, and shortcuts**
  - **Depends:** Task 25.
  - Match welcome panel, breadcrumb, warnings, action rows, compact collapse,
    bordered composer, model badge, dynamic multiline growth, startup-to-draft
    clearing, status/shortcut grammar, cursor/focus, prompt history, slash
    dropdown, file `@` completion, ghost suggestions, and Bash/feedback/remember
    composer modes retained by Task 2.
  - **QA:** state tests, PTY inputs, semantic cells, fixed pixels, bad input,
    paste/Unicode/IME, and restart-persisted settings.
  - **Evidence:** `task-26/tui-shell-composer/`.

- [ ] **Task 27 — Transcript, markdown, syntax, tools, diffs, links, selection, and media**
  - **Depends:** Task 25.
  - Match user/assistant/reasoning/tool/task blocks, streaming/completed/failed/
    retry/cancel states, timing/usage chrome, markdown/tables/code, syntax,
    edit diffs, folding/raw views, hyperlinks, block viewer/copy, scroll/follow,
    character selection, clipboard, local image attachments, inline image/media
    controls, and Mermaid rendering when retained.
  - No voice or hosted media generation affordance may reappear.
  - **QA:** deterministic event fixtures plus live PTY interactions, long content,
    Unicode width, malformed media, terminal fallback, and pixel comparison.
  - **Evidence:** `task-27/tui-transcript/`.

- [ ] **Task 28 — Overlays, permissions, questions, palette, models, sessions, auth, and settings**
  - **Depends:** Tasks 25-27.
  - Match measured overlay sizes/placement/dimming/z-order/preemption, filtering,
    selection, dismissal, mouse hit targets, permission/question flows, command
    palette, shortcuts help, model picker, session picker/tree/fork/clone/rename,
    public auth/account UI, settings editor, theme picker, memory, plan viewer,
    prompt stash, and local extensions/MCP/agents views.
  - Removed enterprise/remote/marketplace actions must be absent, not disabled.
  - **QA:** every entry/exit/error/persist path by keyboard and mouse; overlay
    collision/preemption; PTY and pixels at all required viewports.
  - **Evidence:** `task-28/tui-overlays/`.

- [ ] **Task 29 — Dashboard, agents, tasks, queue, plans, and worktree/session journeys**
  - **Depends:** Tasks 20-22 and 25-28.
  - Implement the retained Grok dashboard interaction model over Harness-native
    coordinator truth: rows, grouping/filtering, selection, peek, rename, stop,
    permissions/questions, auto-approve, worktree choice, task/subagent status,
    queues, plan state, and session navigation. Do not depend on hosted hub state.
  - **QA:** real multi-agent/coordinator journeys, failure/cancel/restart, keyboard
    and mouse, responsive layout, semantic cells, and pixels.
  - **Evidence:** `task-29/tui-dashboard/`.

- [ ] **Task 30 — Vim, minimal/fullscreen, navigation, mouse, and responsive terminal behavior**
  - **Depends:** Tasks 25-29.
  - Match Grok vim editing, mode indicators, minimal/fullscreen relaunch behavior,
    history/find, next/previous turn/response, fold/raw/expand-all, page/half-page,
    focus switching, mouse capture, wheel/trackpad modes, selection/copy-on-select,
    terminal title/progress/focus events, and alternate-screen behavior.
  - **QA:** PTY-driven key/mouse matrices, resize during every mode, reduced-color,
    legacy keys, no-color, non-TTY/error relaunch, and persisted preferences.
  - **Evidence:** `task-30/tui-modes-terminal/`.

- [ ] **Task 31 — Themes, auto/system appearance, notifications, and contextual tips**
  - **Depends:** Task 25.
  - Match all retained named Grok themes and exact token roles, truecolor/basic/
    no-color adaptation, system auto dark/light selection, preview/revert,
    notification timing, terminal title/progress, sleep inhibitor behavior,
    focus-aware permission/background notifications, tips, seen counts, and
    contextual hint dismissal/persistence.
  - **QA:** theme token/cell/pixel matrix, system preference changes, unsupported
    terminal fallback, notification focus timing, and persistence/restart.
  - **Evidence:** `task-31/tui-themes-notices/`.

- [ ] **Task 32 — Canonical TUI action/slash/setting registry cleanup**
  - **Depends:** Tasks 26-31.
  - Ensure every retained visible shortcut, slash command, palette action, setting,
    alias, and mouse action calls exactly one real operation. Remove all excluded
    entries and stale snapshots. Generate coverage directly from registries.
  - **QA:** registry coverage, unknown command/error, no duplicate aliases, no
    fake action, no removed entry, and mutation replacing action dispatch with
    no-op fails.
  - **Evidence:** `task-32/tui-registry.json`.

- [ ] **Task 33 — Wave 3 TUI integration and visual signoff**
  - **Depends:** Tasks 25-32.
  - Integrate shared AppState/layout/theme/render/keybinding roots serially.
    Generate fresh L1-L4 evidence for every TUI manifest row from one integrated
    candidate. No row changes to pass until this task validates it.
  - Build a draft binary from the integrated TUI tree, record its path/SHA/version
     as `task-33-draft-binary.json`, set `HARNESS_TUI_DRAFT_BIN` and `HARNESS_BIN`
     to that exact absolute path, and use it for Task 33 PTY/native captures. This
     is not the final installed candidate;
    Task 35 rematerializes after Task 34 seals the product epoch.
  - **QA:** all harness-tui owners, deterministic render, PTY, native terminal,
    xterm.js semantic cells/pixels, responsive matrix, accessibility/contrast,
    and removed-surface absence.
  - **Evidence:** `task-33/wave-3-tui-signoff.json`.

### Wave 4: truthful manifests, live provider, and installed-binary dogfood

- [ ] **Task 34 — Recompute all inventories from the integrated candidate**
  - **Depends:** Tasks 24 and 33.
  - Re-run capability, subsystem, TUI, command/action, provider, config, and
    scope-removal inventories as a read-only candidate draft. Freeze the
    `product_epoch` and candidate inputs; do not promote checked-in statuses yet.
    Record external blockers precisely; internal residuals stay incomplete.
  - **QA:** full manifest mutation suite plus a product-epoch seal check proving
    all product source bytes are immutable before candidate materialization. This
    task does not require or claim an installed binary; Task 35 owns installation.
  - **Evidence:** `task-34/inventory-draft.json`,
    `task-34/product-epoch-input-set.json`, `task-34/product-epoch-seal.json`.

- [ ] **Task 35 — Live Umans provider/model matrix**
  - **Depends:** Tasks 14, 23, 24, and 34.
  - Materialize one candidate binary into a new isolated install directory from
     the sealed product epoch and verify the exact
     `task-34/product-epoch-input-set.json` digest before building. Record
     source/product epoch, build command/toolchain,
    destination path, executable mode/size, SHA-256, and post-copy SHA-256 before
    setting `HARNESS_BIN`. Reject `cargo run`, a rebuild during testing, or an
    alternate path.
  - Invoke that exact candidate binary directly with `HARNESS_BIN`.
    Emit `candidate-install-seal.json` containing source/product epoch, source
    digest, destination path, executable metadata, binary SHA-256, version, and
    post-copy hash. After this seal, any product-source change invalidates Tasks
    35-42 and requires a new candidate attempt.
    Using secret-safe environment injection and `harness.jsonc`, exercise the
    three unique backend models listed in §1.1 through the real provider/auth
    transport. Verify `umans-coder` resolves to the Kimi backend and
    `umans-flash` resolves to the Qwen backend through config/model resolution;
    do not run duplicate live calls for those aliases.
  - For each unique backend model: config resolution, streaming text, reasoning/variant handling,
    tool call, permission allow and deny, malformed/provider error, cancellation,
    persistence/resume, and redaction.
  - **QA:** missing credential and invalid credential are failures, not skips;
    secret scan of all outputs; candidate path/SHA/version checked before every
    subprocess; a canary bash/hook/ACP/MCP child proves the provider credential
    is absent from its environment, argv, inherited descriptors, and artifacts.
  - **Evidence:** `task-35/live-provider/`.

- [ ] **Task 36 — Agent-controlled installed-binary dogfood**
  - **Depends:** Tasks 24, 33, and 35.
  - In isolated temporary workspaces, use the real Harness agent to: inspect/edit/
    verify a file; deny a mutation then recover; cancel and resume; create/switch/
    clean a worktree; resume/fork/rewind a session; use local MCP or ACP; change a
    setting and prove source attribution; exercise memory/queue/background tasks;
    handle provider error; and process injected plus native sleep/wake events.
  - No direct event injection may be the sole proof for ordinary product journeys.
  - **QA:** run `bash scripts/harness-qa-dogfood.sh --self-test` with explicit
    `HARNESS_BIN` and a new temporary workspace. The dogfood driver must produce
    one receipt per listed journey with prompt, actual tool calls, event hash,
    external postcondition, failure-path result, and workspace before/after hash.
    A fixture that directly writes AppState/events or only prints a success JSON
    is rejected.
  - **Evidence:** `task-36/dogfood/` with prompts, tool calls, events, external
    postconditions, and before/after workspace status.

- [ ] **Task 37 — Installed-binary PTY/native visual acceptance**
  - **Depends:** Tasks 33, 35, and 36.
   - Set `HARNESS_SEALED_BIN` to the absolute installed candidate path from
     `candidate-install-seal.json`. Run `--help`, `--version`, config validate, doctor, offline mock, full TUI
    reference matrix, and live/dogfood journeys through the exact installed
    candidate. Every artifact records candidate/reference identity at creation.
  - **QA:** set `HARNESS_BIN` to the recorded executable and run
    `scripts/test-lanes.sh signoff-binary` and
    `RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 scripts/test-lanes.sh signoff-pty`.
    Verify each receipt names the same candidate path/SHA/version and fresh
    product epoch; any cargo-run or alternate binary invocation fails the task.
  - **Evidence:** `task-37/installed-candidate/`.

### Wave 5: same-revision release gate and independent review

- [ ] **Task 38 — Full deterministic and quality gate from a clean integration worktree**
  - **Depends:** Tasks 34-37.
  - Run, without omission:

    ```bash
    cargo fmt --all -- --check
    cargo check --workspace
    cargo clippy --all-targets --all-features --workspace -- -D warnings
    cargo nextest run --workspace
    scripts/test-lanes.sh fast
    scripts/test-lanes.sh quality-gates
    scripts/test-lanes.sh integration
    scripts/test-lanes.sh all-deterministic
    scripts/test-lanes.sh simulation
    HARNESS_BIN="$HARNESS_SEALED_BIN" scripts/test-lanes.sh signoff-binary
    HARNESS_BIN="$HARNESS_SEALED_BIN" RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 scripts/test-lanes.sh signoff-pty
    HARNESS_BIN="$HARNESS_SEALED_BIN" bash scripts/harness-qa-dogfood.sh --self-test
    ```

  - Add every targeted owner command required by changed crates. All commands
    consume the same source/candidate identity. Pre-existing failures are not
    acceptable unless proven unrelated and explicitly approved by the user.
  - **QA:** capture command, environment names, exit code, stdout/stderr hashes,
    product epoch, candidate SHA, and secret-scan result for every command. Any
    skipped/continued/`|| true` stage fails the task.
  - Enumerate every immutable evidence file permitted as reviewer input, reject
    symlinks/mutable paths/unexpected files, hash the ordered set, and emit
    `task-38/base-evidence-input-set.json`. Task 39 must verify this exact digest
    before reading any artifact.
  - Task 6 owns the resolver/runner schema; Task 38 materializes
    `task-38/resolved-review-command-manifest.json` with no placeholders. Each
    gate entry contains absolute `cwd`, complete argv, non-secret environment,
    isolated target/output roots, every input path plus byte digest, the exact
    candidate/reference/product identities, and the expected output paths. Task
    40 executes only this manifest for F1/F2/F3/F4-pre. Task 41 materializes a
    second resolved manifest after final attestation for F4-final; Task 42
    consumes both manifests and never reconstructs commands from prose.
  - **Evidence:** `task-38/full-gates.json`,
    `task-38/base-evidence-input-set.json`,
    `task-38/resolved-review-command-manifest.json`.

- [ ] **Task 39 — Independent visual/interaction review and holdouts**
  - **Depends:** Tasks 37-38.
  - Reviewer A audits exact Grok source/binary fidelity for shell, overlays,
    themes, input, timing, responsiveness, accessibility, and mouse/keyboard.
  - Run undisclosed holdouts for Unicode width, long content, resize, scroll,
    focus, selection, no-color/basic color, legacy keys, timing perturbation,
    notification focus, and every retained mode.
   - **QA:** run
     `python3 scripts/run-resolved-review.py --manifest "${HARNESS_REVIEW_COMMAND_MANIFEST:?set by Task 38}" --reviewer visual`
     in a fresh read-only process with a distinct reviewer identity. The resolved
     manifest supplies the reference/candidate/base-evidence paths, output paths,
     and every digest; the runner
    must execute the deterministic render/PTY holdouts, verify all input digests,
    emit typed finding IDs with exact cells/traces or a signed unconditional
    approval, and reject missing/ambiguous reviewer provenance. Any finding at
    any severity blocks release.
   - The review seal records `base_evidence_digest`, `product_epoch`, and the
     candidate identity tuple (absolute installed path, binary SHA-256, and
     version), plus reviewer identity/provenance, review content digest, and
     every holdout input digest.
  - **Evidence:** `task-39/visual-review.json`, `task-39/visual-review-seal.json`.

- [ ] **Task 40 — Independent runtime/security/evidence review and final stop gate**
  - **Depends:** Tasks 34-38. It runs in parallel with Task 39; Task 41 consumes
    both independent review seals.
  - Reviewer B audits coordinator authority, replay purity, permissions, sandbox,
    native power events, auth/provider construction, cancellation, persistence,
    cleanup, removed-surface absence, secret handling, evidence freshness,
    dependency/write-set history, manifests, and same-candidate identity.
  - Run F1 plan compliance, F2 code quality/security, F3 manual QA, and F4
    pre-promotion scope/evidence fidelity as independent executable gates. The
    pre-promotion F4 uses Task 34 draft manifests, Task 38 base evidence, and
    Task 40’s own runtime evidence; it does not require Task 39/41 outputs or
    final promoted statuses.
   - **QA:** run
     `python3 scripts/run-resolved-review.py --manifest "${HARNESS_REVIEW_COMMAND_MANIFEST:?set by Task 38}" --reviewer runtime-security-evidence`
     in a fresh read-only process with a distinct reviewer identity. The resolved
     manifest supplies the reference/candidate/base-evidence paths, runtime
     evidence output, review output, seal output, and every digest; the runner
    executes F1/F2/F3 and pre-seal F4 against the immutable runtime evidence
    output before emitting `runtime-review-seal.json`; it then verifies the seal
    schema. The pre-seal validator must consume `runtime-evidence.json`, never
    the seal it is about to create. The runner verifies all input digests and
    emits typed root-cause findings or a signed unconditional approval. No prose
    “looks good” is accepted.
  - A rejection emits a typed root cause and reopens the earliest owner task plus
    its integration/capture descendants in a fresh repair namespace. Final review
    infrastructure defects reopen their owning evidence task. Never patch a final
    receipt in place.
   - The runtime review seal records the same `base_evidence_digest`, `product_epoch`,
     and candidate identity tuple as the visual seal, plus its reviewer
     identity/provenance, runtime review content digest, F1-F4 receipts, and all
     security/evidence input digests. Task 41 must compare these fields byte-for-
     byte before promotion.
   - **Evidence:** `task-40/final-review.json`, `task-40/runtime-review-seal.json`.

### 7.1 F1-F4 executable review routing

Task 6 must create the shared validator entrypoint and resolved-command manifest
runner. Task 38 must set `HARNESS_REVIEW_COMMAND_MANIFEST` to the absolute path
of its immutable, fully resolved manifest; Task 41 must set
`HARNESS_FINAL_REVIEW_COMMAND_MANIFEST` to the equivalent final-review manifest.
Task 40 invokes only these manifests from frozen product/evidence roots. Task 42
derives `task-42/resolved-terminal-command-manifest.json` from both sealed
manifests, preserving input digests while remapping every output and temp root
under `task-42/qa/**`; it never reconstructs commands from prose. The gate names
and runner are part of the next-loop contract; if a runner is absent, the owning
task creates it before any release task can pass:

| Gate | Exact command | Owner and inputs | Expected result and repair routing |
|---|---|---|---|
| F1 plan compliance | `python3 scripts/run-resolved-review.py --manifest "${HARNESS_REVIEW_COMMAND_MANIFEST:?set by Task 38}" --gate F1` | Task 6/38; resolved manifest contains the absolute plan path, ledger, evidence root, reservation ledger, task receipts, and all input digests | zero dependency/write-set/timestamp/receipt violations; findings reopen Task 6 or the earliest named product task and invalidate descendants |
| F2 code quality/security | `python3 scripts/run-resolved-review.py --manifest "${HARNESS_REVIEW_COMMAND_MANIFEST:?set by Task 38}" --gate F2` | Task 4/15/38; resolved manifest contains exact source checkout, candidate seal, child-env and sandbox fixtures, and output roots | every command exits zero; findings reopen Task 4, 14, 15, 16, or earliest product owner according to the root-cause field |
| F3 manual/product QA | `python3 scripts/run-resolved-review.py --manifest "${HARNESS_REVIEW_COMMAND_MANIFEST:?set by Task 38}" --gate F3` | Task 37/38; resolved manifest contains the absolute sealed candidate path/SHA/version, fresh workspace, TUI/reference receipts, and isolated outputs | installed candidate, PTY, native and dogfood journeys all pass; findings reopen the named product owner and invalidate Tasks 35-42 |
| F4 pre-promotion scope/evidence fidelity | `python3 scripts/run-resolved-review.py --manifest "${HARNESS_REVIEW_COMMAND_MANIFEST:?set by Task 38}" --gate F4-pre` | Task 4/5/40; resolved manifest contains draft manifests, source/reference/candidate seals, base evidence, immutable pre-seal Task 40 runtime evidence, and exact mutation fixtures | no stale/copy/self-comparison/scope resurrection; findings reopen Task 5, 34, or earliest owner before final promotion |
| F4 final scope/evidence fidelity | `python3 scripts/run-resolved-review.py --manifest "${HARNESS_FINAL_REVIEW_COMMAND_MANIFEST:?set by Task 41}" --gate F4-final` | Task 41/42; resolved manifest contains final manifests, Task 39/40 review seals, both epochs, exact candidate/reference identities, and isolated mutation fixtures | no stale/copy/self-comparison/status inflation/scope resurrection; findings reopen Task 5, 34, 41, or earliest owner and invalidate final attestation |

Every rejection receipt contains `root_cause_class`, `earliest_task`, exact
write reservation, invalidated product/attestation/evidence identities, affected
descendant tasks, fresh repair namespace, and re-entry gate. A reviewer finding
without this schema is itself an F1 evidence failure and routes to Task 6.

- [ ] **Task 41 — Final attestation status promotion after all evidence**
  - **Depends:** Tasks 39-40.
  - **Write set:** attestation manifests and final evidence indexes only; no
    product source, generated source, Cargo input, or candidate binary bytes.
   - Recompute every status from the sealed `product_epoch`, Task 38 base evidence
     seal, Task 39 visual review seal, Task 40 runtime review seal, exact
     applicability rules, and approved scope ledger. Promote rows
     only now. Record `product_epoch`, `attestation_epoch`, candidate binary SHA,
     reference SHA, manifest/scenario digests, and every blocker/divergence.
     Before promotion, compare `base_evidence_digest`, `product_epoch`, and the
     candidate path/SHA-256/version tuple across both review seals and the Task 35
     candidate-install seal; any mismatch reopens the earliest owning task.
  - **QA:** materialize the resolved final-review command manifest, then run
     `python3 scripts/run-resolved-review.py --manifest "${HARNESS_FINAL_REVIEW_COMMAND_MANIFEST:?set by Task 41}" --gate F4-final`
     twice, once against the final attestation and once with a deliberate
     stale/copy/self-comparison mutation; first passes, second fails. No earlier
     draft status is reused.
  - **Evidence:** `task-41/final-attestation.json`,
    `task-41/resolved-final-review-command-manifest.json`.

- [ ] **Task 42 — Final read-only consistency and release stop**
  - **Depends:** Task 41.
   - **Write set:** `task-42/release-stop.json`,
     `task-42/oracle-input-set.json`,
     `task-42/resolved-terminal-command-manifest.json`, and isolated
     `task-42/qa/**` outputs only.
  - Verify the root plan task ledger, dependency ledger, salvage ledger, product
    epoch, attestation epoch, candidate binary, source/reference identities,
    manifests, all L0-L6 receipts, absence tests, and final workspace receipts
    agree. This task cannot repair source or statuses; any mismatch reopens the
    owning task in a new repair namespace.
   - **QA:** run, in a fresh read-only validation pass, `git diff --check`,
     `cargo fmt --all -- --check`, `cargo check --workspace`,
     `cargo clippy --all-targets --all-features --workspace -- -D warnings`,
     `cargo nextest run --workspace`, `scripts/test-lanes.sh fast`,
     `scripts/test-lanes.sh quality-gates`, `scripts/test-lanes.sh integration`,
     `scripts/test-lanes.sh all-deterministic`, `scripts/test-lanes.sh simulation`,
     `HARNESS_BIN="$HARNESS_SEALED_BIN" scripts/test-lanes.sh signoff-binary`,
     `HARNESS_BIN="$HARNESS_SEALED_BIN" RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 scripts/test-lanes.sh signoff-pty`,
     `HARNESS_BIN="$HARNESS_SEALED_BIN" bash scripts/harness-qa-dogfood.sh --self-test`,
      the task-42-scoped resolved F1/F2/F3/F4 command manifest from §7.1, and the final secret
     scan. Every command must set its `cwd`, target directory, temporary
     workspace, evidence root, and output paths under the isolated Task 42 QA
     root; no command may write the product checkout or checked-in statuses.
     Expected result: zero open findings, no stale/copy evidence, and exact stop
     eligibility.
   - Enumerate, hash, and seal the exact §7.2 terminal-oracle inputs as
     `task-42/oracle-input-set.json`; reject symlinks, missing files, mutable
     directories, or identities from another attempt.
   - After the implementer claims this receipt complete, OMO invokes its single
     terminal `subagent_type=oracle` review against the full loop, not against
     an individual task. The loop may stop only on an unconditional oracle
     approval; the oracle writes no workspace or task evidence.
   - **Evidence:** `task-42/release-stop.json`,
     `task-42/oracle-input-set.json`,
     `task-42/resolved-terminal-command-manifest.json`, isolated
     `task-42/qa/**` outputs.

### 7.2 OMO terminal oracle contract

After the Task 42 implementer returns its completion claim, the OMO lead invokes
the built-in orchestration reviewer exactly once with a direct call equivalent
to:

```text
task(
  subagent_type="oracle",
  description="Terminal Grok parity completion review",
  load_skills=["karpathy-guidelines", "programming", "rust-best-practices"],
  run_in_background=false,
  prompt=f"Review the immutable oracle input set at {absolute_oracle_input_set_path}; do not edit the workspace or write task evidence. Return the required terminal-oracle verdict schema."
)
```

The OMO lead substitutes the absolute path recorded by Task 42 before invoking
the call; it is not a runtime placeholder. Its ordered, immutable records must include the root-plan
path and SHA-256, source/reference seal and source-tree digest, candidate-install
seal and binary SHA-256, `product-epoch-input-set.json` and `product_epoch`,
`attestation_epoch`, Task 38 base-evidence input-set and resolved-command
manifest, Task 39/40 review seals and resolved manifests, Task 41 final
attestation and resolved-final-review manifest, Task 42 release-stop receipt,
resolved-terminal-command manifest, the final secret-scan receipt, and the
complete L0-L6 receipt index. Each record includes absolute path, byte SHA-256,
and the identity it belongs to; the aggregate digest is recorded in the Task 42
receipt.

OMO accepts only this machine-readable result shape in its orchestration
metadata:

```json
{
  "schema_version": 1,
  "verdict": "unconditional_approval",
  "reviewer": {"subagent_type": "oracle", "session_id": "...", "model": "..."},
  "input_set_digest": "sha256:...",
  "product_epoch": "sha256:...",
  "attestation_epoch": "sha256:...",
  "candidate_sha256": "sha256:...",
  "reference_sha256": "sha256:...",
  "read_only": true,
  "findings": []
}
```

`verdict` must be exactly `unconditional_approval`, `findings` must be empty,
`read_only` must be true, reviewer provenance must be present, and every digest
must equal the corresponding immutable input. A rejection uses
`"verdict":"rejected"` plus typed findings; prose, missing provenance, a
missing input digest, a mutation, or any open finding is not a release verdict.
The oracle is an OMO reviewer, not a `.agent-harness/agents/` project profile,
and it writes no workspace, task receipt, status manifest, or evidence file.

If the terminal oracle rejects, OMO records the rejection against the current
attempt, invalidates Task 34 through Task 42 candidate/attestation/release
identities and every descendant of the reported `earliest_task`, and creates a
new `repair-r<M>` namespace. Only verified pre-repair digests that the finding
does not invalidate may be carried into the repair receipt. The scheduler then
re-enters at the earliest owner, reruns its integration and every descendant
through Task 42, regenerates the oracle input set, and invokes a fresh terminal
oracle review. No rejected Task 41/42 status or receipt may remain eligible for
promotion, and no final receipt is patched in place.

## 8. Dependency and parallelism summary

```text
Wave 0: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7
Wave 1: 7 -> {8,9,10,11,12,15}; 9 -> 14 -> 16; 12+15 -> 13;
        {8,9,10,11,12,13,14,15,16} -> 17
Wave 2: 17 -> {18,19,20,21,22}; {13..22} -> 23 -> 24
Wave 3: {2,4,24} -> 25 -> {26,27,31}; {26,27} -> 28;
        {20,21,22,25,26,27,28} -> 29 -> 30;
        {26..31} -> 32 -> 33
Wave 4: {24,33} -> 34; {14,23,24,34} -> 35;
        {24,33,35} -> 36; {33,35,36} -> 37
Wave 5: {34,35,36,37} -> 38 -> {39,40} -> 41 -> 42
```

Safe parallel writer groups are Tasks 8/9/10/11/12/15, Tasks 18/19/20/21/22,
Tasks 26/27/31 after Task 25, and independent reviewers 39/40 after Task 38.
Tasks 17, 24, 33, 34, 38, 41, and 42 are exclusive integration/gate tasks. Task
41 is the only final status promoter; Task 42 is read-only and cannot repair a
failing receipt. After the Task 42 implementer claim, OMO runs one terminal
`subagent_type=oracle` review outside this task graph; it is read-only, writes no
task evidence, and its unconditional approval is the final stop gate.

## 9. Final completion criteria

The loop may stop only when all conditions hold on one exact source revision and
one exact installed candidate:

- Every retained Grok public source surface is represented in the crosswalk and
  has an implemented Harness action or an exact user-approved divergence.
- Every excluded feature is absent from source dependencies, config/schema,
  capability inventories, CLI help, slash/palette/actions/settings, TUI, docs,
  tests, and runtime network behavior.
- Every retained visible TUI row has matching semantic cells, settled pixels,
  fixed animation ticks, focus/cursor, timing, responsive behavior, keyboard,
  mouse, terminal fallback, error, cancellation, restart, and recovery evidence.
- Every advertised command/action reaches the real authority and has a meaningful
  failure path and observable postcondition.
- Native OS sleep/wake sources and credential-refresh guards are operational or
  truthfully externally blocked on the tested platform; the injectable source is
  not accepted as the product implementation.
- Every supported provider claim is config-reachable; the three unique Umans
  backend models pass the live matrix and Coder/Flash aliases resolve correctly;
  optional providers are explicitly labeled
  `auth-boundary`/offline-only unless separately authorized for live proof; all
  persisted provider metadata is redacted.
- Installed-binary agent dogfood passes through real product surfaces.
- All applicable L0-L6 evidence is fresh, immutable, secret-clean, and tied to
  the same source/reference/manifest/scenario/candidate identities.
- All deterministic, quality, integration, simulation, binary, PTY, live, and
  dogfood gates pass.
- Independent visual and runtime/security/evidence reviews have zero open
  findings at any severity.
- Final status promotion occurs only in Task 41 after Tasks 35-40, and Task 42
  verifies the final attestation without mutating it.
- The OMO terminal `subagent_type=oracle` reviews the complete Task 1-42 loop
  after the Task 42 implementer claim and returns unconditional approval.
- The source checkout and isolated test workspaces satisfy their expected final
  cleanliness receipts, and unrelated user changes remain preserved.

Otherwise the loop continues. A green build, passing subset, polished reskin,
complete registry, unavailable result, mock run, copied capture, or agent claim
is never a completion condition.

## 10. Next-loop start instruction

Start at **Task 1**. Do not resume Todo 13, the old 39-task run, the missing
`.omo/plans` target, or any prior checkpoint. The first loop response must name
the new attempt ID, evidence root, current branch/HEAD, reference identity,
`HARNESS_PARITY_PLAN` pointing to this root file, and the exact stop condition
for Task 1 before any source edit occurs.

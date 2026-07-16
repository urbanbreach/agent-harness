# Harness Live & Residual Agent Testing PRD

**Status:** Planning / implementation-ready. No workstream is complete until the
progress ledger cites concrete evidence. Unchecked items stay unchecked.

**Date:** 2026-07-16  
**Last accuracy polish:** 2026-07-16 (authored against closed enhancement PRD +
deferred residual inventory)

**Audience:** Autonomous implementation agents working this PRD in a loop, plus
reviewers who must reject premature completion claims.

**Authority:** Subordinate to:

- root [`AGENTS.md`](../AGENTS.md)
- [`docs/AGENTS.md`](./AGENTS.md)
- [`docs/testing.md`](./testing.md)
- [`docs/claim-evidence-matrix.md`](./claim-evidence-matrix.md)
- [`docs/release-blockers.md`](./release-blockers.md)
- [`docs/privacy-and-local-data.md`](./privacy-and-local-data.md)
- [`docs/provider-support.md`](./provider-support.md)
- crate-scoped `AGENTS.md` for every crate touched
- runtime invariants in [`crates/harness-core/AGENTS.md`](../crates/harness-core/AGENTS.md)
- live policy in [`crates/harness-testkit/tests/README.live-proxy.md`](../crates/harness-testkit/tests/README.live-proxy.md)

If this PRD conflicts with runtime invariants, lane ownership rules (especially
**T5 live slimming**), or privacy redaction policy, **those win**. Update this
PRD with a dated note; do not adapt around invariants silently.

**Progress ledger:**
[`docs/harness-live-agent-testing-progress.md`](./harness-live-agent-testing-progress.md)

**Predecessor (complete; do not re-open as incomplete):**
[`docs/harness-testing-enhancement-prd.md`](./harness-testing-enhancement-prd.md)
+ [`docs/harness-testing-enhancement-progress.md`](./harness-testing-enhancement-progress.md)

---

## 0. Read this first

### 0.1 Governing objective

Ship the **residual** testing capabilities other harnesses already treat as
first-class — especially **budgeted live smoke with redacted artifacts**,
**opt-in live agent/skill dogfood**, **skill activation under the real task path**,
**optional local free live**, **cassette edge residual**, and **offline chaos
residual** — adapted to Harness’s coordinator-centric, event-sourced design so
they are **effective without lying**.

Priorities (in order):

1. **Preserve offline ownership** — deterministic nextest + simulation + offline
   `harness-qa` remain the primary proof of runtime/tool behavior.
2. **Live honesty** — live proves transport / auth / parity / fixed smoke only;
   never re-owns the native tool behavioral matrix (**T5**).
3. **Agent-usable channels** — one runtime skill (`harness-qa`) with clear
   offline vs live channels + scripts agents can run with isolation and evidence.
4. **Fail-closed claims** — no ledger `verified` without commands, artifacts,
   and (where required) skeptical review.
5. **Secret safety** — redaction + secret scan before any live evidence claim.

### 0.2 What “residual live/agent testing” means here

| Dimension | Meaning |
|-----------|---------|
| **Channel taxonomy** | Offline dogfood (shipped), live-smoke (opt-in), skill-activation (offline-first) — not a free-for-all eval farm. |
| **Live smoke pack** | Fixed short prompts + optional one env-safe tool path; budgets; redacted artifacts; fail-closed without env. |
| **Live agent dogfood** | Agents drive real harness with live provider under isolation + budgets; evidence dirs; never CI-required. |
| **Skill-in-loop** | Real `skill` / `task(load_skills=…)` resolve/activate paths proven offline first; live skill-load smoke optional. |
| **Cassette residual** | Deterministic transport edges still thin after enhancement PRD (abort mid-tool-call, richer chunk boundaries). |
| **Chaos residual** | Concurrent child stress, corrupt-events beyond matrix negatives, mid-flight permission deny — offline only. |
| **Scenario growth** | Owner-nextest-first; no default multi-scenario simulation admission; no mega-scenarios. |
| **Claim integrity** | Live claim classes require env provenance + redacted artifacts + non-claims. |

### 0.3 Non-goals

- Do not edit anything under `inspirations/`.
- Do not copy foreign branding, package layout, or UI copy.
- Do not re-implement offline `harness-qa` / `scripts/harness-qa-dogfood.sh` as if
  missing (predecessor PRD **verified**).
- Do not re-own the native tool / runtime behavioral matrix via live tests (**T5**).
- Do not make freestyle live missions / open-ended eval batches a CI or release
  requirement (**REJECT for V1 of this PRD** — D4).
- Do not require Docker isolation for V1 (D3 deferred).
- Do not make replay execute tools, network, hooks, or providers.
- Do not weaken, delete, ignore, or rubber-stamp tests.
- Do not store raw requests/responses, auth headers, cookies, keys, PEM blocks,
  or hidden reasoning text in events / artifacts / evidence (privacy + core
  invariants). Live session events may retain local reasoning-delta evidence
  where already allowed; that is not public support material.
- Do not default-admit new offline-deterministic simulation matrix scenarios
  without measured expected_predicates (D6 / scenario policy A).
- Do not treat this PRD as complete because docs were written; implementation
  without verification is incomplete.

### 0.4 Implementation-agent operating rules (loop protocol)

Skipping a step is a process failure even if code “looks done.”

#### Loop step A — Orient

1. Read root `AGENTS.md` and every crate `AGENTS.md` for files you will touch.
2. Before coding, load `karpathy-guidelines` and `programming` (plus matching
   skills). Distinct from **runtime** skills under `.agent-harness/skills/`.
3. Re-read `docs/testing.md`, live-proxy README, and this PRD’s current workstream.
4. Open only inspiration paths listed for the **current workstream** (§2.2).
5. Confirm predecessor offline dogfood still green before claiming residual live
   work does not regress it.

#### Loop step B — Ledger before code

Before the first code edit in a workstream, **append** a progress-ledger row
with status `planning` or `in_progress` containing:

- Workstream id (e.g. `WS-L1`, `WS-L2`)
- Inspiration paths actually opened (repo-relative)
- Harness source paths expected to change
- Behavior summary in the agent’s own words
- Intended tests / dogfood / live scenarios
- Explicit non-claims for this slice

#### Loop step C — TDD / fail-first

- Behavioral work: failing owner test / script self-test **before** production.
- Live work: first prove **fail-closed without env**, then green path with env.
- Process/docs-only: explicit `unchecked` / `documented_limitation` / disposition
  instead of fake green tests.

#### Loop step D — Smallest invariant-preserving slice

- One workstream at a time (or offline-parallel L3/L5/L6 only).
- Do not start a later dependent workstream while acceptance gates fail.
- **Independent exceptions:** inventory notes; docs stubs; pure path audits.

#### Loop step E — Verify

Capture exit codes, artifact roots, focused nextest names, dogfood/live evidence
paths, secret-scan results.

#### Loop step F — Dogfood / real surface

Product-touching slices need a real surface:

- Offline: `bash scripts/harness-qa-dogfood.sh --self-test` (must stay green)
- Live: env-gated script/lane only when claiming live classes
- Never claim live from offline alone

#### Loop step G — Ledger after code

**Append a superseding row** (do not rewrite history). Statuses from §5.1 only.
Terminal `verified` requires §0.5 false for every item.

#### Loop step H — Stop conditions

- After 3 failed fix attempts on the same root cause: stop, document, consult
  Oracle/review.
- Inspiration vs Harness conflict → prefer Harness; `rejected` / `deferred`.
- Missing live credentials → `blocked_external`; do not invent live green.

### 0.5 Premature-completion guardrails (hard)

An agent **must not** claim a workstream or this PRD is complete if any of:

1. Progress ledger lacks verification commands and observed results.
2. Required owner tests were not run (or `--ignored` skipped when required).
3. Docs claim a lane / skill / scenario without owner test or script coverage.
4. Simulation matrix / claim-evidence / live docs drifted without matching tests.
5. Live / PTY / native claims lack artifact provenance.
6. Dogfood or live evidence dir missing, empty, or contains secrets.
7. `scripts/test-lanes.sh quality-gates` or required closeout lanes fail.
8. Status set to `verified` without skeptical review when **Requires review**.
9. Historical ledger rows rewritten instead of superseded.
10. “Done” argued from source inspection alone without execution evidence.
11. Live smoke “artifacts” claimed without implementing them (baseline slim
    wrappers still write **no** live artifact trees until WS-L1 lands).
12. Live tests reassert offline-owned tool matrix behavior.
13. Freestyle mission/eval CI claimed as required.

**Forbidden phrases without evidence paths:**

- “fully tested”
- “live coverage complete”
- “agents can now dogfood live end-to-end” (without evidence path + env provenance)
- “parity with inspirations complete”
- “all scenarios covered”
- “signoff ready” / “release ready”
- “tool matrix proven live”

Use precise claims: scenario id, channel name, lane name, artifact root, budget
ids, invariant ids.

### 0.6 Definition of done for this PRD

**Two layers** (do not confuse them):

#### Authoring DoD (docs-only task that creates this PRD)

- This PRD file + progress ledger exist and are accurate.
- `docs/AGENTS.md` points here as the **active** residual testing loop.
- Branding allowlist covers these filenames if they cite inspirations.
- `scripts/test-lanes.sh quality-gates` green after authoring.
- Offline dogfood not regressed (optional re-run).
- **No** implementation workstream marked `verified`.

#### Implementation DoD (agents implementing WS-L*)

Complete only when **all** hold:

1. **WS-L8** and **WS-L9** are `verified` (claim integrity + freestyle REJECT
   documented in public testing map as implementation lands).
2. **WS-L10** process wiring `verified` for residual channels that shipped.
3. **WS-L1, WS-L2, WS-L3, WS-L4, WS-L5, WS-L6, WS-L7** each dispositioned
   (`verified` / `blocked_external` / `deferred` / `rejected`) with evidence or
   dated rationale.
4. **At least one of WS-L1 or WS-L2** is `verified` when live env is available;
   otherwise honest `blocked_external` / `deferred` with docs non-claims — do
   **not** claim “live residual complete.”
5. Offline dogfood remains green:
   `bash scripts/harness-qa-dogfood.sh --self-test`
6. T5 non-ownership of the tool matrix restated in testing.md / live-proxy README
   when live work lands.
7. Closeout commands green with paths in ledger:

   ```bash
   scripts/test-lanes.sh quality-gates
   scripts/test-lanes.sh fast
   scripts/test-lanes.sh simulation
   scripts/test-lanes.sh all-deterministic
   ```

8. No secret-bearing evidence committed; no live/PTY/native overclaim.
9. Skeptical review for workstreams marked **Requires review**.
10. Predecessor enhancement PRD not rewritten; residual scope only.

---

## 1. Baseline facts (verify again before editing)

These are source-audited starting observations, **not** completion claims.

### 1.1 Existing strengths (predecessor + core)

| Capability | Status | Anchor |
|------------|--------|--------|
| Offline agent dogfood skill + script | **Shipped / verified** | `.agent-harness/skills/harness-qa/`, `scripts/harness-qa-dogfood.sh` |
| Offline theme owners (permissions, multi-tool, compaction, lineage, provider error, session inspect) | **Verified** | `docs/testing.md` theme table; enhancement progress |
| Simulation matrix offline-deterministic | **`golden_path` only** | `docs/simulation-matrix.json` |
| Live lane `signoff-live` | **T5 slim** preflight + parity names | `live_proxy_e2e.rs`, live-proxy README |
| Live wrappers write live artifact trees | **No** (baseline) | live-proxy README “Artifact layout” |
| Stress live | Env-gated stress script | `scripts/stress-harness.sh --mode live` |
| Skill discovery V1 | Strong | `skill_load_discovery_test` |
| Task + `load_skills` | Runtime + owner tests exist | harness-tools native task tests |
| Cassette / stream edges | Substantial; residual listed | `harness-providers` tool_errors / mock |
| Hashline overlap + matrix negatives | Strong | core/tools + simulation matrix |
| Claim-evidence matrix + quality-gates | Strong | `docs/claim-evidence-matrix.md` |

### 1.2 Gaps this PRD targets

| Gap | Symptom |
|-----|---------|
| No budgeted live smoke **artifact pack** | Slim wrappers produce no `run-*` / manifest trees |
| No opt-in **live agent dogfood** channel | `harness-qa` is offline-only by design |
| Skill multi-activation under real loop not productized as QA recipe | Owner tests exist piecemeal; no agent-facing recipe |
| Optional local free live (Ollama) | Docs deferral only |
| Cassette residual | Abort mid-tool-call corpus; richer multi-chunk; mock Error fields |
| Chaos residual | Concurrent child stress under mock; corrupt-events beyond matrix; mid-flight deny stress |
| Live claim classes underspecified | Easy to overclaim “live works” from preflight alone |
| Freestyle missions | Inspiration has eval/missions; Harness must **reject as CI proof** |

### 1.3 Ownership rules that must not regress

- Deterministic owners own behavioral tool / runtime matrices.
- `signoff-live` does **not** own broad provider / tool-flow behavior.
- Simulation owns offline behavioral E2E for **admitted** scenarios only
  (currently `golden_path`).
- PTY / native remain separate provenance classes.
- Session inspection remains replay-derived and side-effect free.
- Coordinator is sole event append / permission / lifecycle authority.
- Offline `harness-qa` remains mandatory for product-touching changes per root
  `AGENTS.md`; **live dogfood stays opt-in** (D7).

---

## 2. Look-here maps

### 2.1 Primary local anchors

| Concern | Path |
|---------|------|
| Lane map / live non-claims | `docs/testing.md` |
| Claim evidence | `docs/claim-evidence-matrix.md` |
| Live proxy policy | `crates/harness-testkit/tests/README.live-proxy.md` |
| Live tests | `crates/harness-testkit/tests/live_proxy_e2e.rs` |
| Lane runner | `scripts/test-lanes.sh` (`signoff-live`, `stress-live`) |
| Offline dogfood skill | `.agent-harness/skills/harness-qa/SKILL.md` |
| Offline dogfood script | `scripts/harness-qa-dogfood.sh` |
| Evidence convention (offline) | `.agent-harness/skills/harness-qa/references/evidence-convention.md` |
| Skill discovery | `crates/harness-tools/tests/skill_load_discovery/` |
| Task / load_skills | harness-tools native task / agent spawn tests |
| Mock + cassette | `crates/harness-providers/src/mock.rs`, `cassette.rs` |
| Scenarios | `crates/harness/src/scenarios.rs` |
| Simulation matrix | `docs/simulation-matrix.json` |
| Stress | `scripts/stress-harness.sh` |
| Privacy / redaction | `docs/privacy-and-local-data.md` |
| Provider support / Ollama note | `docs/provider-support.md` |
| Agents / task | `docs/agents-and-subagents.md` |
| Predecessor PRD | `docs/harness-testing-enhancement-prd.md` |

### 2.2 Inspiration trees (read-only; never edit)

| Pattern | Look here first |
|---------|-----------------|
| Multi-channel agent QA + evidence | `inspirations/senpi/.agents/skills/senpi-qa/` (SKILL.md, `scripts/mock-loop.mjs`, `rpc-drive.mjs`, `tui-smoke.mjs`, `cli-smoke.mjs`, isolation helpers) |
| Product QA process + isolation | `inspirations/oh-my-openagent/.agents/skills/opencode-qa/`, `…/codex-qa/` |
| Live harness + budgets + redaction + artifacts | `inspirations/pi_agent_rust/docs/testing-policy.md`; `tests/e2e_live_harness.rs`; `tests/run_e2e.sh`; `tests/common/harness.rs` |
| VCR / cassette edges | `inspirations/pi_agent_rust/tests/agent_loop_vcr.rs`; `tests/vcr_redaction_scan.rs` |
| Scenario runner / tmux | `inspirations/pi_agent_rust/tests/common/scenario_runner.rs`; `tests/common/tmux.rs` |
| Freestyle missions (**default REJECT for CI**) | `inspirations/oh-my-codex/missions/`; `inspirations/oh-my-codex/src/scripts/eval/` |

Re-open paths in the ledger every phase; do not implement from memory.

### 2.3 Target channel architecture

```text
                     ┌──────────────────────────────┐
                     │ Owner nextest + simulation   │
                     │ (behavioral matrix ownership)│
                     └──────────────┬───────────────┘
                                    │
          ┌─────────────────────────┼─────────────────────────┐
          ▼                         ▼                         ▼
 ┌─────────────────┐    ┌────────────────────┐    ┌───────────────────┐
 │ Channel offline │    │ Channel live-smoke │    │ Channel skill-act │
 │ harness-qa      │    │ harness-qa live +  │    │ offline multi-    │
 │ dogfood.sh      │    │ live-smoke.sh      │    │ skill load path   │
 │ (SHIPPED)       │    │ budgets+artifacts  │    │ (+ optional live) │
 └─────────────────┘    └────────────────────┘    └───────────────────┘
          │                         │                         │
          └─────────────────────────┼─────────────────────────┘
                                    ▼
                     claim-evidence + progress ledger
                     (fail-closed; T5 non-ownership)
```

| Channel | Env | Primary proof | Does **not** prove |
|---------|-----|---------------|--------------------|
| **offline** | none | Mock multi-step wiring + events | Live providers; PTY pixels |
| **live-smoke** | `HARNESS_LIVE_PROXY*` | Transport/auth/parity/fixed smoke + redacted artifacts | Tool matrix; freestyle quality |
| **skill-activation** | none (live optional) | Skill resolve / multi-skill before spawn | Skill “quality” evals |

---

## 3. Workstreams

### WS-L0 — Process shell, taxonomy, look-here (docs)

**Goal:** This PRD’s process layer, channel taxonomy, and maps are accurate so
implementers need no interview.

**Acceptance (authoring):** PRD §0–§2 complete; channels named; T5 + offline
baseline explicit.

**Non-claims:** No product code.

---

### WS-L1 — Live smoke pack (budgeted, redacted artifacts)

**Goal:** Opt-in live lane / script exercises a **fixed short smoke list** with
artifact provenance, cost/time discipline, secret scan, and explicit non-
ownership of tool matrices.

**Baseline reminder:** Slim wrappers currently write **no** live artifact trees.
WS-L1 must **add** evidence capability (or document a different evidence path)
without reclaiming the tool matrix.

**Look here (Harness):** live-proxy README; `live_proxy_e2e.rs`;
`scripts/test-lanes.sh` `signoff-live`; privacy docs; provider redaction.

**Look here (inspiration):** pi `e2e_live_harness` / `run_e2e.sh` / cost budgets /
redaction scans.

**Recommended fixed smoke list (starter — D8):**

1. Preflight env/config/provider/model tuple (existing).
2. One short non-tool prompt (“reply with the single word PONG”) via real
   prompt/run path under isolated session-dir.
3. Optional: one env-safe tool path (e.g. read a fixture file in sandbox) that
   **must not** be documented as matrix ownership.

**Recommended budgets (starter — D9):**

| Budget | Starter default |
|--------|-----------------|
| Max model turns | 1–3 |
| Prompt size | short fixed strings only |
| Wall clock | hard cap (e.g. 120s per smoke) |
| Cost | if usage available: fail/warn thresholds; else document “unmetered” |
| Secrets | hard fail on `sk-`, `Bearer `, PEM, etc. |

**Artifact shape (live dogfood / smoke):**

```text
artifacts/qa-evidence/<YYYYMMDD>-live-<slug>/
  README.md                 # WHAT/OBSERVED/WHY/OMITTED + non-claims
  commands.log
  isolation-receipt.txt
  budget-receipt.txt        # turns/time/cost if available
  events-excerpt.jsonl      # redacted
  secret-scan.txt
  lane-or-run-summary.txt
```

Lane stage artifacts may also land under `target/test-lanes/<run-id>/…` for
`signoff-live` (D1).

**Acceptance:**

- Fail-closed without live env (script + tests).
- With env: redacted evidence + secret-scan clean **or** honest
  `blocked_external` / `deferred`.
- Docs state what live does **not** prove.
- **Requires review** before `verified`.

**Non-claims:** Full multi-provider matrix; agent freestyle quality; ownership of
native tool behavioral matrix.

---

### WS-L2 — Live agent/skill dogfood channel

**Goal:** Extend runtime skill `harness-qa` with an **opt-in live channel** that
agents can run after product-touching changes when live env is present — without
making live mandatory.

**Design (locked):**

- Keep offline channel behavior and non-claims unchanged.
- Add skill sections: Use When (live), Do Not Use When, Steps invoking
  `scripts/harness-qa-live-smoke.sh` (D2).
- Isolation: session roots under evidence or `/tmp`; never pollute developer
  global harness home/config.
- Evidence under `artifacts/qa-evidence/<date>-live-<slug>/`.
- Missing env → non-zero exit; do not soft-skip into “success.”

**Look here:** existing `harness-qa` skill; dogfood script patterns; senpi-qa
isolation/auth-guard ideas (adapt, do not copy).

**Acceptance:**

- Skill discoverable; offline self-test still green.
- Live mode fail-closed + green path when env present (or disposition).
- Discovery / quality contract tests updated if skill body grows sections.
- **Requires review** before `verified`.

**Non-claims:** Not a substitute for offline dogfood; not simulation ownership;
not tool matrix ownership; not CI default.

---

### WS-L3 — Skill activation under real task/skill path

**Goal:** Productize offline multi-skill `load_skills` / skill activation proof
(owner nextest first) and optional env-safe live skill-load smoke.

**Look here:** `skill_load_discovery_test`; native task tests with `load_skills`;
`docs/starter-skills.md`; `docs/agents-and-subagents.md`.

**Acceptance:**

- Multi-skill resolve order, dedupe, missing/disabled fail-before-spawn covered
  by owner tests **or** explicit disposition.
- Optional dogfood recipe (offline) documenting how agents verify skill load.
- Live skill-load smoke dispositioned (verify / defer / reject).
- No default matrix admission.

**Non-claims:** Not freestyle skill quality scoring.

---

### WS-L4 — Optional local free live (e.g. Ollama)

**Goal:** Document/script an optional zero-or-low-cost local provider path for
dogfood, without making it CI-required.

**Look here:** `docs/provider-support.md`, `docs/config.md`, live env vars.

**Acceptance:** Documented optional path + fail-closed when unavailable; explicit
“not CI default.” May remain `deferred` without blocking other streams.

**Non-claims:** Not production support promise; not `signoff-live` default.

---

### WS-L5 — Cassette / VCR edge residual

**Goal:** Close residual deterministic stream edges:

- Abort mid-tool-call cassette/fixture corpus
- Richer multi-chunk boundary cases beyond dual-delta
- Categorized mock `Error` fixture fields beyond plain message

**Look here:** `harness-providers` mock/cassette/tests; quality-gates cassette
secret hygiene; prior enhancement progress residual list.

**Acceptance:** New fixtures + owner nextest green **or** remaining gaps listed
as `deferred`.

**Non-claims:** Live model quality; TUI.

---

### WS-L6 — Chaos offline residual

**Goal:** Strengthen fail-closed offline controls still residual:

- Concurrent child stress under mock
- Corrupt-events chaos beyond matrix `negative_controls`
- Permission-deny mid-flight stress scenarios

**Look here:** simulation matrix negatives; hashline owners; `stress-harness.sh`;
coord permission/lifecycle tests.

**Acceptance:** New controls with docs **or** residual list deferred. No live
dependency. Prefer owner nextest / stress stages over matrix mega-scenarios (D10).

**Non-claims:** Not live chaos.

---

### WS-L7 — Declarative scenario growth policy (disciplined)

**Goal:** Encode how to grow `scenarios.rs` / CLI scenarios without mega-scenarios
and without default simulation multi-admission.

**Policy (locked A):**

1. Prefer focused owner nextest over matrix admission.
2. New CLI scenarios OK when they have owners.
3. Matrix `planned` rows optional; `offline-deterministic` admission only after
   measured expected_predicates and simulation-lane update plan.
4. Never grow `golden_path` into an unmaintainable mega-scenario.

**Acceptance:** Policy in PRD (this section) + later docs/testing.md pointer when
implementation changes scenarios.

**Non-claims:** Not automatic matrix growth.

---

### WS-L8 — Live claim integrity

**Goal:** Extend claim taxonomy and (when implementation lands) claim-evidence
rows for:

| Claim class | Allowed when |
|-------------|--------------|
| Live transport / parity | `signoff-live` env present + redacted preflight/parity evidence |
| Live smoke pack | Fixed smoke list green + artifact root + secret scan + budgets receipt |
| Live agent dogfood | Skill/script live channel + isolation + evidence dir + env provenance |
| Live skill-load smoke | Explicit env-safe smoke + non-claims |

**Acceptance:** PRD table (now); claim-evidence rows + testing.md language when
channels ship; **Requires review** for public phrases.

**Non-claims:** Not release-ready.

---

### WS-L9 — Explicit REJECT freestyle missions (V1)

**Goal:** Document that open-ended live freestyle eval missions (oh-my-codex
missions/eval style) are **rejected as CI / release proof** for V1. Local human
experimentation is fine; it is not evidence for PRD boxes.

**Acceptance:** Non-goals + this workstream + testing.md note when process
wiring lands. No CI requirement language.

**Non-claims:** Does not ban local freestyle; bans treating it as proof.

---

### WS-L10 — Docs / process wiring for residual channels

**Goal:** When residual channels ship, update together:

- `docs/testing.md` (live smoke pack, live dogfood, non-claims)
- `docs/claim-evidence-matrix.md`
- `docs/starter-skills.md` / skill quality docs
- live-proxy README artifact section
- root `AGENTS.md` only if D7 expands (default: live stays opt-in)
- docs-reference / lane tests as needed

**Authoring phase of this PRD:** `docs/AGENTS.md` active-loop pointer + branding
allowlist only.

**Requires review** before claiming residual process complete.

---

## 4. Evidence conventions

### 4.1 Progress ledger

File: `docs/harness-live-agent-testing-progress.md`

| Date | Commit | Workstream | Inspiration sources opened | Harness sources touched | Tests / lanes run | Dogfood evidence | Status | Notes |

Rules: append-only supersede; §5.1 statuses only; evidence path must exist at
write time or `blocked_external` with missing env named.

### 4.2 QA evidence directories

| Class | Root |
|-------|------|
| Offline dogfood (shipped) | `artifacts/qa-evidence/<YYYYMMDD>-<slug>/` |
| Live dogfood / live smoke | `artifacts/qa-evidence/<YYYYMMDD>-live-<slug>/` |
| Lane stages | `target/test-lanes/<run-id>/…` |

Do not commit secrets. Prefer gitignored `artifacts/`.

### 4.3 Secret safety

Before claiming live/dogfood complete: secret scan on evidence; cassette gates
where relevant; never paste API keys into ledger.

---

## 5. Status model and claim taxonomy

### 5.1 Progress status values

| Status | Meaning |
|--------|---------|
| `planning` | Ledger note only |
| `in_progress` | Active implementation |
| `implemented` | Code landed; verification incomplete |
| `verified_pending_review` | Evidence ready; waiting review |
| `verified` | Gates green; review done if required |
| `blocked_external` | Missing credentials/env/hardware |
| `deferred` | Explicitly out of current completion set |
| `rejected` | Considered and intentionally not done |
| `failed` | Attempted; not green |

No `complete` / `harness_adapted` statuses.

### 5.2 Claim classes

| Claim class | Allowed when |
|-------------|--------------|
| Deterministic behavioral | Owner nextest / simulation admission |
| Agent dogfood offline | Skill + evidence + isolation (shipped) |
| Live transport / parity | Env + redacted parity/preflight evidence |
| Live smoke pack | Fixed list + artifacts + budgets + secret scan |
| Live agent dogfood | Live channel + isolation + evidence + env provenance |
| Binary / PTY / native | Existing lane provenance rules |

---

## 6. Suggested implementation order

```text
[Authoring — this PRD]
  L0 shell + L8/L9 text + L10 AGENTS pointer

[Implementation]
  Wave 1 (parallel offline): L5 cassette ∥ L6 chaos ∥ L3 skill multi-load
  Wave 2 (live): L1 live smoke pack + script → L2 skill live channel
  Wave 3 (optional): L4 Ollama; L7 scenarios only as needed
  Wave 4: L8 claim rows + testing.md + live-proxy README (L10)
  Closeout: disposition all; offline dogfood green; quality-gates/fast/simulation/all-deterministic
```

---

## 7. Required verification catalog (minimum)

| Slice | Minimum verification |
|-------|----------------------|
| Offline dogfood regression | `bash scripts/harness-qa-dogfood.sh --self-test` |
| Skill / discovery | `cargo nextest run -p harness-tools --test skill_load_discovery_test` |
| Live fail-closed | Script/test exit non-zero without `HARNESS_LIVE_PROXY=1` |
| Live smoke (when claimed) | Env-gated run + artifact path + secret scan |
| Cassette residual | focused `harness-providers` nextest |
| Chaos residual | owner nextest / `stress-offline` |
| Docs claims | `config_docs_reference_test` as needed |
| Simulation | `scripts/test-lanes.sh simulation` (golden_path) |
| Always before PRD complete | `quality-gates`, `fast`, `simulation`, `all-deterministic` |
| PTY / native | only when claiming those classes |

---

## 8. Deferred decisions (must be dispositioned, not ignored)

| ID | Decision | Locked default |
|----|----------|----------------|
| D1 | Live evidence root | Dogfood: `artifacts/qa-evidence/<date>-live-<slug>/`; lane stages: `target/test-lanes/…` |
| D2 | Live script | `scripts/harness-qa-live-smoke.sh` invoked by skill |
| D3 | Docker isolation | **Deferred** V1 |
| D4 | Freestyle missions CI | **REJECT** V1 |
| D5 | Ollama first-class optional | Documented non-CI; impl may defer |
| D6 | New matrix invariant ids | Reuse INV-001…004; no new IDs by default |
| D7 | Project AGENTS live mandate | Offline stays mandatory for product-touching; **live opt-in only** |
| D8 | Fixed smoke list | Preflight + short prompt + optional one tool |
| D9 | Budgets | 1–3 turns; short prompts; wall-clock cap; cost if measurable; secret hard-fail |
| D10 | Concurrent child stress home | Owner nextest / stress stage, not matrix mega-scenario |

Default bias: **prefer deferred over speculative infrastructure**; prefer
**scripts + skill + scenarios** over new permanent services.

---

## 9. Anti-patterns specific to this PRD

- Implementing live by shelling into senpi/OMO products instead of dogfooding
  **agent-harness**.
- Adding live tests that reassert tool matrix behavior already owned offline.
- Claiming live smoke “artifacts” without implementing them.
- Making live dogfood CI-required without env (fails closed must stay).
- Growing `golden_path` into a mega-scenario.
- Treating freestyle missions as release evidence.
- Committing evidence/cassettes with secrets.
- Confusing **project** coding skills with **runtime** skills.
- Rewriting the completed enhancement PRD to invent backdated live completion.

---

## 10. Final acceptance checklist (reviewer / Oracle)

- [ ] Offline dogfood still green (predecessor not regressed)
- [ ] WS-L1 dispositioned (`verified` / `blocked_external` / `deferred` / `rejected`)
- [ ] WS-L2 dispositioned
- [ ] WS-L3…L7 dispositioned
- [ ] WS-L8 `verified` (claim integrity for shipped live classes)
- [ ] WS-L9 `verified` (freestyle REJECT in public testing map)
- [ ] WS-L10 process wiring for what shipped
- [ ] T5 non-ownership restated wherever live claims appear
- [ ] `quality-gates`, `fast`, `simulation`, `all-deterministic` green (paths in ledger)
- [ ] No secret-bearing evidence committed
- [ ] Live / PTY / native claims have provenance or are absent
- [ ] Progress ledger append-only; skeptical review for **Requires review**
- [ ] README / public docs do not over-claim

---

## 11. First actions for a fresh implementer agent

1. Confirm progress ledger exists; append `planning` for WS-L1 or first offline
   residual (L5/L6/L3).
2. Re-read live-proxy README, `docs/testing.md` live section, `harness-qa` skill,
   predecessor deferred rows.
3. Open pi live harness + senpi-qa isolation patterns (read-only).
4. Prefer offline Wave 1 (L5/L6/L3) if live env missing; do not claim live green.
5. For live: implement fail-closed tests first, then smoke pack + script, then
   skill channel.
6. Do not mark `verified` until §0.5 is satisfied.

---

## 12. Document maintenance

| Change | Also update |
|--------|-------------|
| Live smoke pack / artifacts | live-proxy README, `docs/testing.md`, claim-evidence, lane script tests |
| Live dogfood skill channel | `.agent-harness/skills/harness-qa/`, starter-skills, skill discovery tests |
| New live script | `scripts/`, scripts AGENTS, skill resources |
| Cassette residual | providers AGENTS, quality-gates cassette hygiene |
| Chaos residual | testing.md stress section, stress script tests |
| Scenario growth | `scenarios.rs`, simulation matrix **only if admitted**, testing.md |
| Claim phrases | claim-evidence-matrix + docs-reference tests |

Update this PRD with dated notes when scope changes; disposition deferred items
in the ledger — do not erase them.

---

## 13. Relationship to enhancement PRD + accuracy log (2026-07-16)

| Predecessor item | Residual mapping |
|------------------|------------------|
| WS-P0 offline dogfood **verified** | Baseline (do not re-open) |
| WS-P1 theme owners **verified** | Baseline |
| WS-P6 process **verified** | Baseline |
| WS-P2 live smoke **deferred** | **WS-L1** |
| WS-P3 cassette residual **deferred** | **WS-L5** |
| WS-P4 Ollama **deferred** | **WS-L4** |
| WS-P5 chaos residual **deferred** | **WS-L6** |
| New: live agent/skill dogfood | **WS-L2** |
| New: skill multi-activation recipes | **WS-L3** |
| New: scenario growth policy | **WS-L7** |
| New: live claim classes | **WS-L8** |
| New: freestyle REJECT explicit | **WS-L9** |

Path audit at authoring: senpi-qa scripts under
`inspirations/senpi/.agents/skills/senpi-qa/scripts/`; pi live harness +
testing-policy present; live wrappers still write no artifact trees; offline
`harness-qa` present and offline-only.

Re-audit inspiration/Harness paths before treating this section as stale if the
tree moves.

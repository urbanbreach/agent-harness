# Harness Testing Enhancement PRD

**Status:** Planning / implementation-ready. No workstream is complete until the
progress ledger cites concrete evidence. Unchecked items stay unchecked.

**Date:** 2026-07-16  
**Last accuracy polish:** 2026-07-16 (path audit + DoD alignment; see §13)

**Audience:** Autonomous implementation agents working this PRD in a loop, plus
reviewers who must reject premature completion claims.

**Authority:** Subordinate to:

- root [`AGENTS.md`](../AGENTS.md)
- [`docs/AGENTS.md`](./AGENTS.md)
- [`docs/testing.md`](./testing.md)
- [`docs/claim-evidence-matrix.md`](./claim-evidence-matrix.md)
- [`docs/release-blockers.md`](./release-blockers.md)
- [`docs/privacy-and-local-data.md`](./privacy-and-local-data.md)
- crate-scoped `AGENTS.md` for every crate touched
- runtime invariants in [`crates/harness-core/AGENTS.md`](../crates/harness-core/AGENTS.md)

If this PRD conflicts with runtime invariants, lane ownership rules, or privacy
redaction policy, **those win**. Update this PRD with a dated note; do not adapt
around invariants silently.

**Progress ledger:**
[`docs/harness-testing-enhancement-progress.md`](./harness-testing-enhancement-progress.md)

---

## 0. Read this first

### 0.1 Governing objective

Make agent-harness testing as strong as the best patterns in `inspirations/`,
adapted to Harness architecture:

1. **Deterministic offline behavioral ownership** remains the primary proof of
   runtime features (simulation matrix, mock scenarios, owner nextest).
2. **Agent dogfood** becomes a first-class, isolated, evidence-backed process:
   agents exercise the real binary/CLI (and TUI when needed) after product
   changes, not only unit tests.
3. **Live provider proof** stays opt-in, slim, budgeted, redacted, and
   non-owning of behavioral tool/runtime matrices (**preserve T5 live
   slimming** — see `crates/harness-testkit/tests/README.live-proxy.md`).
4. **Claims stay fail-closed**: no completion, no roadmap checkmarks, no README
   claims without ledger rows + verification commands + artifacts.

### 0.2 What “enhanced testing” means here

| Dimension | Meaning |
|-----------|---------|
| **Lane integrity** | Existing lanes still map correctly; new lanes/stages are documented and script-tested. |
| **Scenario coverage** | Offline mock scenarios cover more than the single matrix-admitted `golden_path`. |
| **Agent QA process** | A shipped skill + scripts make offline dogfood mandatory and reproducible for product-touching changes. |
| **Evidence culture** | Progress ledger + claim-matrix rows; no “it worked for me.” |
| **Live honesty** | Live proves transport/auth/parity smoke (and any **new** smoke pack this PRD adds) only; does not re-own the tool matrix. |
| **Secret safety** | Artifacts, cassettes, evidence dirs, support bundles stay redacted and scanned. |
| **Inspiration adaptation** | Patterns from OMO / senpi / pi_agent_rust / etc. are adapted into Harness terms — not copied as foreign package layout. |

### 0.3 Non-goals

- Do not edit anything under `inspirations/`.
- Do not copy foreign branding, package layout, or UI copy.
- Do not move runtime invariants into CLI / tools / providers / TUI crates.
- Do not make replay execute tools, network, hooks, or providers.
- Do not make live LLM freestyle exploration a CI requirement or the primary
  feature proof.
- Do not undo T5: live wrappers must not reclaim broad behavioral ownership from
  deterministic owners (`docs/testing.md`, live-proxy README).
- Do not claim PTY / live / native visual proof without matching lane +
  provenance.
- Do not weaken, delete, ignore, or rubber-stamp tests to pass gates.
- Do not store raw requests/responses, auth headers, cookies, keys, PEM blocks,
  or hidden reasoning text in events / artifacts / evidence (root invariants +
  privacy docs). Live session events may retain local reasoning-delta evidence
  where already allowed; that is not public support material.
- Do not treat this PRD as complete because docs were written; implementation
  without passing verification is incomplete.

### 0.4 Implementation-agent operating rules (loop protocol)

Skipping a step is a process failure even if code “looks done.”

#### Loop step A — Orient

1. Read root `AGENTS.md` and every crate `AGENTS.md` for files you will touch.
2. Before any coding edit, load skills required by root AGENTS:
   `karpathy-guidelines` and `programming` (and any other skills that match the
   slice). This is **project coding guidance**, distinct from **runtime** skills
   under `.agent-harness/skills/`.
3. Re-read `docs/testing.md` ownership rules for the surface you are changing.
4. Open the inspiration paths listed for the **current workstream only** (§3).
   Do not implement from chat memory of those systems.

#### Loop step B — Ledger before code

Before the first code edit in a workstream, **append** a progress-ledger row
with status `planning` or `in_progress` containing:

- Workstream id (e.g. `WS-P0`, `WS-P1-theme-permissions`)
- Inspiration paths actually opened (repo-relative)
- Harness source paths expected to change
- Behavior summary in the agent’s own words
- Intended tests / dogfood scenarios
- Explicit non-claims for this slice

#### Loop step C — TDD / fail-first

- Behavioral work: write or extend a failing owner test / matrix assertion /
  script test **before** production changes.
- Process/docs-only slices: make claim status explicit (`unchecked` /
  `documented_limitation`) instead of inventing a green test that proves
  nothing.
- Prefer the narrowest nextest filter or lane that owns the surface.

#### Loop step D — Smallest invariant-preserving slice

- One workstream (or one theme card under WS-P1) at a time.
- Do not start a later workstream while the current workstream’s acceptance and
  dogfood gates are failing.
- **Independent exceptions (may parallelize):** inventory-only notes; docs stubs
  that do not claim completion; pure path audits.

#### Loop step E — Verify

Run the verification commands for that workstream. Capture:

- exit codes
- artifact roots (`scripts/test-lanes.sh … --artifact-dir …` when a lane is
  required)
- focused nextest names
- dogfood evidence directory paths

#### Loop step F — Dogfood through the real surface

Unit tests alone are insufficient for workstreams that touch run / prompt / TUI /
lanes / skills. Drive at least one real surface:

- `cargo run -p harness -- …` or the built binary
- mock / deterministic scenario path
- or the env-gated signoff lane that owns the claim

Record exact commands under §4.

#### Loop step G — Ledger after code

**Append a superseding row** (do not rewrite history). Use only status values from
§5.1. Terminal `verified` requires §0.5 to be false for every item.

#### Loop step H — Stop conditions

- After 3 failed fix attempts on the same root cause: stop, document attempts,
  consult Oracle/review; do not shotgun.
- If an inspiration pattern conflicts with Harness invariants: prefer Harness;
  disposition as `rejected` or `deferred` with rationale in Notes (do not invent
  a status named `harness_adapted`).
- If live credentials/env are missing: `blocked_external`; do not claim live
  proof.

### 0.5 Premature-completion guardrails (hard)

An agent **must not** claim a workstream or this PRD is complete if any of the
following are true:

1. Progress ledger row lacks verification commands and observed results.
2. Required owner tests were not run (or ignored tests were skipped when the
   owning lane requires `--ignored`).
3. Docs claim a lane / skill / scenario that has no owner test or script
   coverage.
4. Simulation matrix / TUI manifest / claim-evidence matrix drifted without
   matching validator / test updates.
5. Live / PTY / native claims lack artifact provenance paths.
6. Dogfood evidence directory is missing, empty, or contains secrets.
7. `scripts/test-lanes.sh quality-gates` or the workstream’s required lanes fail.
8. Status was set to `verified` without a skeptical review when the workstream
   is marked **Requires review** in §3.
9. Historical ledger rows were rewritten instead of superseded.
10. “Done” is argued from source inspection alone without execution evidence.
11. Live proof is claimed from the current slim wrappers alone without new smoke
    evidence this PRD requires (see §1.1 live baseline).

**Forbidden phrases in ledger / PR notes without evidence paths:**

- “fully tested”
- “agents can now dogfood end-to-end”
- “live coverage complete”
- “parity with inspirations complete”
- “all scenarios covered”
- “signoff ready” / “release ready”

Use precise claims: scenario id, lane name, artifact root, invariant ids.

### 0.6 Definition of done for this PRD

Complete only when **all** hold:

1. **WS-P0**, **WS-P1**, and **WS-P6** are `verified` in the progress ledger.
2. **WS-P2** is `verified`, or `blocked_external` / `deferred` / `rejected` with
   dated rationale and docs non-claims (live enhancement is not silently
   skipped).
3. **WS-P3**, **WS-P4**, and **WS-P5** are each dispositioned
   (`verified` / `deferred` / `rejected`) with evidence or explicit deferral.
4. `docs/testing.md`, `docs/claim-evidence-matrix.md`, and any new skill docs are
   updated together with owners/tests (docs-reference tests pass where
   applicable).
5. Closeout commands succeed on a clean tree (or documented pre-existing
   failures unrelated to this work, listed explicitly):

   ```bash
   scripts/test-lanes.sh quality-gates
   scripts/test-lanes.sh fast
   scripts/test-lanes.sh simulation
   scripts/test-lanes.sh all-deterministic
   ```

6. New agent QA skill is discoverable under the V1 skill contract and covered by
   skill discovery / contract tests as needed.
7. No live / PTY / native claim exists without provenance.
8. A final skeptical review (Oracle or human) has not found open blockers for the
   claimed completion set.

---

## 1. Baseline facts (verify again before editing)

These are source-audited starting observations, **not** completion claims.

### 1.1 Existing strengths

- Canonical lanes via `scripts/test-lanes.sh`: `fast`, `quality-gates`,
  `integration`, `perf`, `coverage`, `simulation`, `signoff-binary`,
  `signoff-pty`, `signoff-live`, `signoff-native`, `stress-offline`,
  `stress-live`, `all-deterministic`.
- Simulation matrix (`docs/simulation-matrix.json`): invariants INV-001…004;
  admitted scenario **`golden_path` only** (as of PRD polish date).
- Built-in CLI scenarios (`crates/harness/src/scenarios.rs`): `golden_path` and
  `golden_path_interactive` (interactive is **not** currently a simulation-matrix
  scenario).
- Binary smoke (`crates/harness/tests/binary_smoke.rs`) and PTY / native visual
  separation (`pty_e2e.rs`, `native_visual_e2e.rs`).
- Live proxy intentionally slim after T5 (`live_proxy_e2e.rs` ignored tests:
  `live_proxy_preflight_requires_live_env`,
  `live_proxy_prompt_parity_signoff`,
  `live_proxy_e2e_tui_parity_signoff`). **Baseline fact:** slim wrappers
  currently verify env/config prerequisites and do **not** write live artifact
  trees (README.live-proxy.md).
- Claim-evidence matrix + static test-suite gates + secret hygiene gates.
- Shipped skills under `.agent-harness/skills/` with V1 frontmatter.
- Existing dogfood-oriented tests (not a substitute for WS-P0): e.g.
  `crates/harness/tests/dogfood_harness_jsonc_test.rs`,
  `dogfood_harness_jsonc_tool_parity_test.rs`.

### 1.2 Gaps this PRD targets

| Gap | Symptom |
|-----|---------|
| Thin matrix-admitted offline scenario set | Simulation admits only `golden_path` |
| Interactive scenario not matrix-owned | `golden_path_interactive` exists in CLI only |
| No mandatory agent dogfood skill | Agents can stop at unit tests |
| No standardized product QA evidence dir | Ad-hoc `/tmp` dogfood only |
| Live smoke not scenario-packaged | Live proves slim preflight/parity only; no artifact pack |
| No productized mock multi-turn QA channel | Senpi/OMO-style mock-loop not shipped as harness skill/scripts |
| Cassette edge coverage uneven | Transport edges may rely on unit tests only |
| Process not encoded for implementers | Inspiration patterns live only under `inspirations/` |

### 1.3 Ownership rules that must not regress

From `docs/testing.md` and T5 live policy:

- Deterministic owners own behavioral tool / runtime matrices.
- `signoff-live` does not own broad provider / tool-flow behavior.
- Simulation owns offline behavioral E2E for **admitted** scenarios only.
- PTY / native are separate provenance classes.
- Session inspection tools remain replay-derived and side-effect free.

---

## 2. Look-here maps

### 2.1 Primary local testing anchors (re-read before every phase)

| Concern | Path |
|---------|------|
| Lane map and signoff policy | `docs/testing.md` |
| Release claim ↔ evidence | `docs/claim-evidence-matrix.md` |
| Simulation matrix contract | `docs/simulation-matrix.json` |
| TUI signoff manifest | `docs/tui-signoff-manifest.v1.json` |
| Lane runner | `scripts/test-lanes.sh` |
| Lane runner script tests | `crates/harness/tests/test_lanes_script_test.rs` |
| Stress runner | `scripts/stress-harness.sh`, `crates/harness/tests/stress_harness_script_test.rs` |
| Static test-suite gates | `scripts/check-test-suite-gates.py`, `docs/test-suite-conventions-baseline.json` |
| Built-in run scenarios | `crates/harness/src/scenarios.rs` |
| Run / prompt CLI | `crates/harness/src/run.rs`, `crates/harness/src/prompt.rs` |
| Binary smoke | `crates/harness/tests/binary_smoke.rs` |
| Simulation library | `crates/harness-testkit/src/simulation.rs`, `crates/harness-testkit/src/simulation/` |
| Simulation evidence bin | `crates/harness-testkit/src/bin/simulation_evidence.rs` |
| Simulation validator | `crates/harness-testkit/tests/simulation_validator_test.rs`, `tests/support/simulation_validator.rs` |
| Live proxy policy + tests | `crates/harness-testkit/tests/README.live-proxy.md`, `tests/live_proxy_e2e.rs` |
| PTY / native visual | `crates/harness-testkit/tests/pty_e2e.rs`, `tests/native_visual_e2e.rs`; `crates/harness-tui/tests/` |
| Mock provider / cassettes | `crates/harness-providers/src/mock.rs`, `cassette.rs`, `crates/harness-providers/AGENTS.md` |
| Hashline edit invariants | `crates/harness-core/src/edit/hashline.rs` |
| Skill contract | `docs/starter-skills.md`, `.agent-harness/AGENTS.md`, `.agent-harness/skills/*/SKILL.md` |
| Skill discovery tests | `crates/harness-tools/tests/skill_load_discovery/`, `skill_load_discovery_test.rs` |
| Agents / task surface | `docs/agents-and-subagents.md` |
| Existing dogfood tests | `crates/harness/tests/dogfood_harness_jsonc_test.rs`, `dogfood_harness_jsonc_tool_parity_test.rs` |
| Docs-reference tests | `crates/harness/tests/config_docs_reference_test.rs`, `event_docs_reference_test.rs` |
| Example PRD + ledger style | `docs/tools-parity-prd.md`, `docs/tools-parity-progress.md` |

### 2.2 Inspiration trees (read-only; never edit)

Paths below were existence-checked during the 2026-07-16 accuracy polish unless
noted.

| Pattern | Look here first |
|---------|-----------------|
| Agent-driven real-product QA + evidence dirs | `inspirations/oh-my-openagent/AGENTS.md`; `inspirations/oh-my-openagent/.agents/skills/opencode-qa/`; `inspirations/oh-my-openagent/.agents/skills/codex-qa/`; `inspirations/oh-my-openagent/script/agent/qa-docker.sh`; `inspirations/oh-my-openagent/script/qa/` |
| Evidence-gated QA + mock full turn | `inspirations/senpi/AGENTS.md`; **`inspirations/senpi/.agents/skills/senpi-qa/SKILL.md`**; scripts under **`inspirations/senpi/.agents/skills/senpi-qa/scripts/`** (`mock-loop.mjs`, `rpc-drive.mjs`, `tui-smoke.mjs`, `cli-smoke.mjs`, `lib/mock-loop-support.mjs`); references under `…/senpi-qa/references/` |
| OMO component mock providers (optional) | `inspirations/oh-my-openagent/packages/omo-senpi/scripts/qa/` (task e2e + mock-provider helpers) |
| Formal testing policy + unit/VCR/e2e taxonomy | `inspirations/pi_agent_rust/docs/testing-policy.md`; `inspirations/pi_agent_rust/AGENTS.md` |
| Live provider E2E + budgets/redaction patterns | `inspirations/pi_agent_rust/tests/e2e_live_harness.rs`; `tests/e2e_live.rs`; `tests/run_e2e.sh`; `tests/common/harness.rs` |
| VCR / cassette agent loop | `inspirations/pi_agent_rust/tests/agent_loop_vcr.rs`; `src/vcr.rs`; `tests/fixtures/vcr/`; `tests/vcr_redaction_scan.rs` |
| Golden path / golden corpus / release-binary E2E | `inspirations/pi_agent_rust/tests/e2e_golden_path.rs`; `tests/e2e_golden_transcript_diff.rs`; `tests/golden_corpus/`; **`examples/ext_release_binary_e2e.rs`**; `tests/release_evidence_gate.rs` |
| Declarative CLI/TUI scenario runner + tmux | `inspirations/pi_agent_rust/tests/common/scenario_runner.rs`; `tests/common/tmux.rs` |
| Faux provider suite harness | `inspirations/pi-mono/packages/coding-agent/test/suite/harness.ts`; `inspirations/senpi/packages/coding-agent/test/suite/harness.ts` |
| HTTP cassette / recorded LLM tests | `inspirations/opencode/packages/http-recorder/`; `inspirations/opencode/packages/llm/test/recorded-*.ts` |
| Wiremock / app-server integration style | `inspirations/codex/codex-rs/core/tests/common/test_codex.rs`; `inspirations/codex/codex-rs/app-server/tests/`; `inspirations/codex/scripts/mock_responses_websocket_server.py` |
| PTY e2e patterns | `inspirations/grok-build/crates/codegen/xai-grok-pager/tests/pty_e2e/`; local `crates/harness-testkit/tests/pty_e2e.rs` |
| Mission/eval batch runners (optional; default bias reject for V1 — D4) | `inspirations/oh-my-codex/src/scripts/eval/`; `inspirations/oh-my-codex/missions/` |

These anchors are **not enough to implement from memory**. Every phase must
re-open current Harness source and the relevant inspiration files, then record
exact paths in the progress ledger.

### 2.3 Target architecture (capabilities only)

```text
                    ┌─────────────────────────────┐
                    │  Owner nextest (T1–T3)      │
                    │  quality-gates / fast        │
                    └──────────────┬──────────────┘
                                   │
          ┌────────────────────────┼────────────────────────┐
          ▼                        ▼                        ▼
 ┌─────────────────┐    ┌──────────────────┐    ┌───────────────────┐
 │ Offline scenarios│    │ Agent QA skill   │    │ Opt-in signoff    │
 │ + simulation     │    │ mock-loop dogfood│    │ PTY / live / native│
 │ matrix           │    │ + evidence dir   │    │ slim + budgeted   │
 └─────────────────┘    └──────────────────┘    └───────────────────┘
          │                        │                        │
          └────────────────────────┼────────────────────────┘
                                   ▼
                    claim-evidence + progress ledger
                    (fail-closed completion)
```

| Target capability | Inspiration | Local integration surface |
|-------------------|-------------|---------------------------|
| Mandatory agent QA + evidence | OMO opencode-qa / codex-qa; senpi-qa | `.agent-harness/skills/`, root/`docs` AGENTS (layer carefully), `docs/starter-skills.md` |
| Isolated sandbox homes | OMO XDG / `CODEX_HOME`; senpi-qa isolation helpers | CLI env / session roots; testkit workspace helpers |
| Mock full-turn loop | senpi-qa `mock-loop.mjs`; pi VCR agent loop | mock provider + `scenarios.rs` + run CLI |
| Scenario matrix growth | pi golden corpus; local simulation matrix | `scenarios.rs` + `docs/simulation-matrix.json` + testkit validator |
| Live smoke pack | pi `e2e_live_harness` / `run_e2e.sh` | `signoff-live` + live-proxy README + `live_proxy_e2e.rs` (**must add** smoke + artifacts if claiming them) |
| Cassette redaction | pi `vcr_redaction_scan`; local cassette gates | `harness-providers` cassettes + `check-test-suite-gates.py` |
| CLI/TUI scripted drive | pi scenario_runner + tmux; local PTY | harness-testkit / harness-tui PTY |
| Release binary proof | pi `examples/ext_release_binary_e2e.rs`; local `signoff-binary` | `binary_smoke.rs` + lane |

---

## 3. Workstreams

Each workstream lists: goal, look-here maps, acceptance evidence shape, and
review requirement. **How** is left to the implementer.

### WS-P0 — Agent dogfood skill, mock-loop channel, evidence convention

**Goal:** Agents can (and, by project instruction where D7 so decides, must for
product-touching changes) exercise the real harness offline via a documented QA
channel and write reviewable evidence.

**Look here (Harness):** §2.1 skill rows, scenarios, run/prompt, mock provider,
existing dogfood tests, `docs/testing.md`, root `AGENTS.md` (only if D7 requires
project-level mandate — keep runtime skills vs project AGENTS layers separate).

**Look here (inspiration):** senpi-qa skill + scripts (§2.2); OMO opencode-qa /
codex-qa skills; OMO `qa-docker.sh` only if D3 is accepted.

**Acceptance (evidence required):**

- Shipped skill package discoverable under V1 rules.
- Documented isolation rules (temp/session roots; no pollution of developer
  global state).
- At least one offline mock multi-step path that exercises real tool/runtime
  wiring and leaves inspectable events/artifacts.
- Evidence directory convention documented; example evidence produced once.
- Owner tests and/or docs-reference guards so the skill does not silently vanish.
- Progress ledger row with commands + paths.
- **Requires review** before `verified`.

**Non-claims:** Does not prove live providers; does not replace simulation
matrix ownership.

---

### WS-P1 — Expand offline scenarios + simulation matrix

**Goal:** Offline deterministic behavioral coverage grows beyond the single
matrix-admitted `golden_path`, with matrix invariants and validator ownership.

**Look here (Harness):** `scenarios.rs`; `docs/simulation-matrix.json`; testkit
simulation + validator; `docs/testing.md` simulation section;
`docs/architecture.md`; `docs/sessions-and-replay.md`; harness-core coord tests
(permissions, compaction, task lifecycle); `scripts/test-lanes.sh` simulation
stage; hashline (`crates/harness-core/src/edit/hashline.rs`).

**Look here (inspiration):** pi golden path / golden corpus / testing-policy
(§2.2).

**Scenario themes (each theme must be dispositioned for WS-P1 `verified`):**

| Theme id | Intent |
|----------|--------|
| `T-permissions` | Permission deny / interactive allow path |
| `T-multi-tool` | Multi-tool or multi-turn tool lifecycle |
| `T-compaction` | Compaction/checkpoint safety (`events.jsonl` not rewritten) |
| `T-task-lineage` | Child task / lineage (or `deferred`/`rejected` if surface insufficient) |
| `T-provider-error` | Provider/stream error → clean terminal state |
| `T-session-inspect` | Session inspection remains side-effect free after a run |

Implementer may merge themes into fewer scenario ids, but **each theme id** needs
an owner test/matrix row **or** an explicit ledger disposition.

**Acceptance:**

- New scenario ids registered in CLI + matrix + validator expectations as needed.
- Same-seed / redaction / invariant rules still fail closed.
- Simulation lane green with artifacts.
- `docs/testing.md` and/or claim-evidence updated for new behavioral claims.
- Progress ledger superseding rows covering every theme id above.
- **Requires review** if matrix schema or invariant ids change.

**Non-claims:** Live provider behavior; TUI pixel fidelity.

---

### WS-P2 — Live smoke pack (slim, budgeted, redacted)

**Goal:** Opt-in live lane exercises a small fixed set of smokes with artifact
provenance, cost/time discipline, and explicit non-ownership of tool matrices.

**Baseline reminder:** Current slim wrappers do **not** write live artifact trees.
If acceptance requires artifacts, this workstream must add that capability (or
document a different evidence path) without re-owning the tool matrix.

**Look here (Harness):** `docs/testing.md` live section; live-proxy README;
`live_proxy_e2e.rs`; `scripts/test-lanes.sh` `signoff-live` / `stress-live`;
privacy docs; support export tests; provider metadata redaction.

**Look here (inspiration):** pi live harness / `run_e2e.sh` / common harness
(§2.2).

**Acceptance:**

- Documented fixed smoke list (short prompts; optional one tool path if
  env-safe and still non-owning of the offline tool matrix).
- Fail-closed when live env missing (preserve current policy).
- If smoke runs succeed: redacted summaries + secret-scan clean **or** explicit
  ledger note that artifacts remain deferred (cannot claim “live smoke pack”
  complete without evidence).
- Docs state what live does **not** prove.
- Ledger: `verified` with env provenance, or `blocked_external` /
  `deferred` / `rejected`.
- **Requires review** before claiming live enhancement complete.

**Non-claims:** Full multi-provider matrix; agent freestyle quality evals;
ownership of native tool behavioral matrix.

---

### WS-P3 — Cassette / VCR edge corpus (transport resilience)

**Goal:** Provider stream edge cases (partial tool calls, abort, error midstream,
chunk boundaries) have deterministic cassette/fixture ownership where
appropriate.

**Look here (Harness):** `crates/harness-providers` (`mock.rs`, `cassette.rs`,
tests); `scripts/check-test-suite-gates.py`; provider AGENTS.

**Look here (inspiration):** pi VCR suite; opencode http-recorder + recorded-*;
codex mock websocket server (§2.2).

**Acceptance:** New/extended fixtures with redaction coverage; owner nextest
green; disposition may be `deferred` with remaining gaps listed.

**Non-claims:** Live model quality; TUI.

---

### WS-P4 — Optional local free live target (e.g. Ollama) docs + scripts

**Goal:** Document/script an optional zero-or-low-cost local provider path for
dogfood, without making it CI-required.

**Look here (Harness):** `docs/provider-support.md`, `configs/`, `docs/config.md`,
live env vars, stress-live.

**Look here (inspiration):** search `inspirations/pi_agent_rust` for ollama /
local live defaults; `examples/ext_release_binary_e2e.rs`.

**Acceptance:** Documented optional path + fail-closed when unavailable;
explicit “not CI default.” May be `deferred` without blocking WS-P0/P1/P6.

---

### WS-P5 — Negative controls / chaos offline expansions

**Goal:** Strengthen fail-closed offline controls (corrupt events, overlapping
edits, permission deny mid-flight, concurrent child stress under mock).

**Look here (Harness):** simulation matrix `negative_controls`; hashline;
`scripts/stress-harness.sh`; coord permission/lifecycle tests.

**Look here (inspiration):** pi `swarm_replay*` / fault-injection fixtures under
`inspirations/pi_agent_rust/tests/`; existing matrix negative controls as the
primary local model.

**Acceptance:** New controls in matrix, owner tests, or stress scripts with docs;
no live dependency; partial completion must list remaining controls.

---

### WS-P6 — Docs, claim matrix, AGENTS process integration

**Goal:** Public testing map and claim evidence reflect the new channels without
over-claiming.

**Look here (Harness):** `docs/testing.md`; `docs/claim-evidence-matrix.md`;
`docs/release-blockers.md`; `docs/AGENTS.md`; docs-reference tests; lane script
tests; root `AGENTS.md` only if D7 accepts a project-level QA mandate.

**Look here (inspiration):** OMO/senpi “NO EVIDENCE == NO QA” process language
(adapt to Harness terms); pi `docs/testing-policy.md` taxonomy language.

**Acceptance:** Docs-reference / lane taxonomy tests green; claim matrix rows for
any new release-facing phrases; no aspirational claims without evidence
pointers; **Requires review.**

---

## 4. Evidence conventions

### 4.1 Progress ledger

File: `docs/harness-testing-enhancement-progress.md`

| Date | Commit | Workstream | Inspiration sources opened | Harness sources touched | Tests / lanes run | Dogfood evidence | Status | Notes |

Rules:

- Append-only; supersede with new rows.
- Status values: §5.1 only.
- Dogfood evidence must be a path that exists at write time (or explicit
  `blocked_external` with missing env named).

### 4.2 QA evidence directories (product dogfood)

Define and document a single convention (implementer picks exact root; must be
consistent). `artifacts/` is gitignored at repo root — preferred for local
evidence.

Suggested shape:

```text
artifacts/qa-evidence/<YYYYMMDD>-<slug>/
  README.md                 # what was tested, non-claims
  commands.log              # exact commands + exit codes
  isolation-receipt.txt     # proves real home/config untouched if applicable
  events-excerpt.jsonl      # redacted / non-secret
  lane-or-run-summary.txt
```

Do not commit secrets. Prefer gitignored roots; committed fixtures only as
redacted goldens under `crates/*/tests/fixtures/`.

### 4.3 Lane artifacts

```text
target/test-lanes/<run-id>/summary.txt
target/test-lanes/<run-id>/<mode>/stages/...
```

PTY visual evidence (when claimed): `target/pty-visual-artifacts/` per testing.md
/ live-proxy README.

Ledger rows that claim lane green must name `summary.txt` paths and PASS/FAIL
counts when available.

### 4.4 Secret safety

Before claiming dogfood/live complete: run applicable secret scans (lane secret
scan, cassette gates, support export tests as relevant). Never paste raw API keys
into ledger or evidence README.

---

## 5. Status model and claim taxonomy

### 5.1 Progress status values

| Status | Meaning |
|--------|---------|
| `planning` | Ledger note only; no code claim |
| `in_progress` | Active implementation |
| `implemented` | Code landed; verification incomplete |
| `verified_pending_review` | Evidence ready; waiting Oracle/human |
| `verified` | Owner tests + required dogfood/docs gates green; review done if required |
| `blocked_external` | Missing credentials/env/hardware |
| `deferred` | Explicitly out of current completion set |
| `rejected` | Considered and intentionally not done |
| `failed` | Attempted; not green; needs follow-up |

Only `verified` (and disposition statuses `deferred` / `rejected` /
`blocked_external` where §0.6 allows) count toward PRD completion accounting.

There is **no** status named `complete` or `harness_adapted`.

### 5.2 Claim classes

| Claim class | Allowed when |
|-------------|--------------|
| Deterministic behavioral | Simulation/owner nextest green + matrix admission for that scenario |
| Agent dogfood offline | Skill + evidence dir + isolation receipt |
| Binary smoke | `signoff-binary` artifact |
| PTY | `signoff-pty` artifact + manifest provenance |
| Live transport / parity / smoke | `signoff-live` env present + redacted evidence for the claim class asserted |
| Native visual | `signoff-native` + `DISPLAY` + provenance |
| Release-ready | Out of scope unless `docs/release-blockers.md` updated with evidence |

---

## 6. Suggested implementation order (not a script)

```text
WS-P0 (agent QA channel)
  └─► WS-P6 (docs/process wiring)  [stubs early OK; finish after P0]

WS-P1 (scenarios + matrix)  [parallelize with P0 if no file conflicts]
  └─► strengthens P0 dogfood recipes

WS-P2 (live smoke) after honest docs from P6; must not own P1 matrix

WS-P3, WS-P4, WS-P5  [parallelizable; may defer]
```

Prefer landing **WS-P0 + WS-P1 theme dispositions + WS-P6** before heavy live
work.

---

## 7. Required verification catalog (minimum)

Run the narrowest set that covers the slice, then broaden before `verified`.

| Slice | Minimum verification |
|-------|----------------------|
| Skill / discovery | `cargo nextest run -p harness-tools --test skill_load_discovery_test`; doctor skill surfaces if applicable |
| Scenarios / run CLI | focused harness scenario/run tests; `scripts/test-lanes.sh simulation` when matrix touched |
| Simulation matrix / validator | `cargo nextest run -p harness-testkit --test simulation_validator_test` |
| Lane runner changes | `cargo nextest run -p harness --test test_lanes_script_test` |
| Docs claims | `cargo nextest run -p harness --test config_docs_reference_test`; `event_docs_reference_test` if events claimed |
| Providers / cassettes | focused `harness-providers` tests + quality-gates cassette hygiene |
| Always before PRD completion claim | `quality-gates`, `fast`, `simulation`, `all-deterministic` |
| PTY / live / native | only when claiming those classes; never fake green by skipping env |

---

## 8. Deferred decisions (must be dispositioned, not ignored)

| ID | Decision |
|----|----------|
| D1 | Whether agent QA evidence dirs are gitignored-only vs optional CI upload |
| D2 | Whether mock-loop is a new CLI subcommand, a scenario family, a script under `scripts/`, or skill-bundled scripts |
| D3 | Whether Docker isolation (OMO-style) is in-scope for V1 of this PRD |
| D4 | Whether open-ended live freestyle eval missions (oh-my-codex missions) are rejected for V1 (**default bias: reject**) |
| D5 | Whether local Ollama is documented as first-class optional live target |
| D6 | Whether new scenarios require new invariant ids vs reusing INV-001…004 with scenario-specific expected vocab |
| D7 | Whether root project `AGENTS.md` must mandate harness-qa for coding agents working **on** this repo (vs runtime skill only) |

Default bias when undecided: **prefer deferred over speculative infrastructure**;
prefer **scripts + skill + scenarios** over new permanent services.

---

## 9. Anti-patterns specific to this PRD

- Implementing OMO/senpi QA by shelling into those products instead of dogfooding
  **agent-harness**.
- Adding live tests that reassert tool matrix behavior already owned offline.
- Growing `golden_path` into an unmaintainable mega-scenario instead of multiple
  named scenarios.
- Checking PRD boxes without ledger evidence.
- Committing cassettes/evidence with secrets, then redacting later.
- Claiming “agents test the harness end-to-end” when only unit tests pass.
- Claiming live smoke “artifacts” without implementing them (baseline has none).
- Using `inspirations/` as a runtime dependency path.
- Editing historical parity/testing PRDs to invent backdated completion.
- Confusing **project** coding skills (`karpathy-guidelines`, `programming`) with
  **runtime** skills under `.agent-harness/skills/`.

---

## 10. Final acceptance checklist (reviewer / Oracle)

Do **not** mark this PRD complete unless every box is honestly checked or
explicitly N/A with rationale:

- [ ] WS-P0 `verified`: skill + mock offline dogfood + evidence convention
- [ ] WS-P1 `verified`: every theme id in §3 dispositioned; matrix + simulation
      lane green for implemented themes
- [ ] WS-P2 `verified` **or** `blocked_external` / `deferred` / `rejected` with
      docs non-claims
- [ ] WS-P3…P5 dispositioned
- [ ] WS-P6 `verified`: testing.md + claim-evidence (+ AGENTS layers as D7 decides)
- [ ] `quality-gates`, `fast`, `simulation`, `all-deterministic` green (paths in
      ledger)
- [ ] No secret-bearing evidence committed
- [ ] Live / PTY / native claims have provenance or are absent
- [ ] Progress ledger has superseding rows; no silent rewrites
- [ ] Skeptical review recorded (session id or human ack) for **Requires review**
      workstreams
- [ ] README / public docs do not over-claim new capabilities

---

## 11. First actions for a fresh implementer agent

1. Confirm progress ledger exists; append `planning` row for WS-P0.
2. Re-read `docs/testing.md`, `docs/simulation-matrix.json`,
   `crates/harness/src/scenarios.rs`, `docs/starter-skills.md`,
   `crates/harness-testkit/tests/README.live-proxy.md`.
3. Open senpi-qa `SKILL.md` + `mock-loop.mjs`, OMO `opencode-qa/SKILL.md`, pi
   `docs/testing-policy.md` (paths in §2.2).
4. Inventory current scenario ids, live test names, and skill pack; paste into
   ledger Notes.
5. Begin WS-P0 fail-first (skill discovery / docs guard / mock path).
6. Do not mark anything `verified` until §0.5 is fully satisfied.

---

## 12. Document maintenance

| Change | Also update |
|--------|-------------|
| New lane or stage | `docs/testing.md`, `scripts/test-lanes.sh`, lane script tests, possibly `docs/release-blockers.md` |
| New simulation scenario/invariant | `docs/simulation-matrix.json`, testkit validator, `scenarios.rs`, testing.md |
| New shipped skill | `.agent-harness/skills/`, `docs/starter-skills.md`, skill discovery tests |
| New release-facing claim | `docs/claim-evidence-matrix.md` + docs-reference tests |
| Live policy change | live-proxy README, testing.md, `live_proxy_e2e.rs` names, privacy notes |

Update this PRD only with dated notes when scope changes; do not erase deferred
items — disposition them in the ledger.

---

## 13. Accuracy polish log (2026-07-16)

Path audit and consistency fixes applied in this revision:

| Issue found | Correction |
|-------------|------------|
| Senpi scripts cited at `inspirations/senpi/scripts/*.mjs` | Actual location: `inspirations/senpi/.agents/skills/senpi-qa/scripts/` |
| `senpi-qa` marked “if present” | Skill exists; paths made definitive |
| pi release-binary cited as fuzzy `tests/*release*binary*` | Concrete: `examples/ext_release_binary_e2e.rs`, `tests/release_evidence_gate.rs` |
| Live “artifacts required” ignored T5 baseline | Documented: slim wrappers currently write **no** live artifact trees; WS-P2 must add evidence if claiming it |
| DoD required WS-P0–P2 but checklist required WS-P6; P2 vs deferred unclear | DoD now requires P0+P1+P6 `verified`; P2 may be deferred/blocked; P3–P5 dispositioned |
| Status `complete` / `harness_adapted` used informally | Removed; only §5.1 statuses allowed |
| WS-P1 checklist said “≥2 themes” vs body listing 6 owners | Verified = every theme id dispositioned |
| Simulation anchors vague | Pointed at `src/simulation.rs`, validator support, evidence bin |
| Mock provider path vague | `mock.rs` / `cassette.rs` |
| PTY paths glob-only | `pty_e2e.rs`, `native_visual_e2e.rs`, `live_proxy_e2e.rs` |
| `golden_path_interactive` omitted from baseline | Noted as CLI-only, not matrix-admitted |
| Existing dogfood tests omitted | Named as baseline, not WS-P0 substitute |
| Loop G allowed “update” historical rows | Clarified append-only supersede |
| Independent work “marked in §3” but unmarked | Explicit independent exceptions in §0.4 D |

Re-audit inspiration/Harness paths before treating this section as stale if the
tree moves.

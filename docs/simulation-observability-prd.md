# Deterministic Simulation and Observability PRD

**Status:** implemented and verified; Sections 12 and 13 closeout evidence is recorded in Section 20.
**Audience:** the implementing agent in a fresh session with no memory of how this document was
produced. Everything required is here or reachable from the paths cited here.
**Mandate:** add an offline deterministic simulation evidence lane that lets agents run the
harness locally and *see behavioral change* through stable artifacts, semantic validators,
same-seed comparisons, and agent-readable reports. The implementation must reuse the current
lane, stress, replay, cassette, and secret-scan infrastructure unless a written comparison proves
that a new helper is necessary.

**Current audit state (2026-05-26):** implementation closeout evidence is recorded in Section 20.
The baseline gap was a missing coherent simulation product with a scenario/invariant matrix,
versioned simulation artifacts, semantic validators, negative controls, same-seed normalized
comparison, and an agent-facing report.

---

## 0. How to use this document (read first)

0.1. This PRD is an implementation contract, not a suggestion list. Every unchecked checkbox is a
requirement unless it is explicitly labelled *Optional* or *Post-MVP*.

0.2. You may not stop until the Definition of Done in Section 13 passes in full and every
acceptance gate in Section 12 has fresh, reproducible evidence from commands you actually ran.
"Looks right" and "the files exist" are not acceptance.

0.3. Preserve runtime invariants. The coordinator remains the sole authority for event append,
scheduling, permission resolution, and tool re-entry. Replay stays pure and side-effect-free.
Event schema remains append-only. See `AGENTS.md`, `crates/harness-core/AGENTS.md`, and
`docs/architecture.md`. If the simulation cannot pass without a product behavior change, stop and
record the reason; do not weaken the invariant.

0.4. Reuse-first rule. Before adding any new runner, helper, fixture corpus, cassette mechanism,
or workflow simulator abstraction, write an implementation comparison note explaining why these
existing surfaces are insufficient or exactly how they are reused:

- `scripts/test-lanes.sh`
- `scripts/stress-harness.sh`
- `crates/harness/src/scenarios.rs`
- `crates/harness/src/run.rs`
- `crates/harness-providers/tests/recorded/*`
- `crates/harness-testkit/src/secret_scanner.rs`
- `crates/harness-testkit/tests/secretscan_test.rs`
- replay/projection owner tests named in `docs/testing.md`

0.5. Negative controls are mandatory. If a validator can only pass happy-path data and has no
checked-in or generated failure case, it is not complete.

0.6. Do not make PTY, live-provider, native screenshot, or visual signoff lanes behavioral owners
for the simulation MVP. Those lanes remain opt-in provenance/signoff evidence only unless a
separate future PRD changes their role.

0.7. Do not publish docs claiming a stable simulation lane until the matrix, schemas, validators,
runner integration, artifacts, and negative controls exist and pass.

---

## 1. Why this work exists

The current harness test suite has strong components but no single deterministic simulation product
that an implementation agent can run after a change to understand what changed.

Current useful pieces:

- `docs/testing.md` documents the canonical lane runner and artifact contract.
- `scripts/test-lanes.sh` records lane-level evidence under an artifact root.
- `scripts/stress-harness.sh` runs offline/live stress stages and records command/stdout/stderr/status/verification files plus `events.jsonl` for prompt/run stages.
- `crates/harness/src/scenarios.rs` contains `golden_path`, `golden_path_interactive`, stable seeded run IDs, deterministic workspace setup, and mock provider request digests.
- `crates/harness-core` owns event/replay/projection invariants.
- `crates/harness-providers/tests/recorded/*` owns provider cassette behavior.
- `crates/harness-testkit` owns fakes, workspace helpers, native visual helper code, and secret scanning.
- `docs/testing.md` names invariant owners for coordinator, replay, permission, tool, provider, UI, and T5 provenance behavior.

The missing product is the connective tissue:

- a checked-in scenario and invariant matrix,
- versioned simulation artifact schemas,
- semantic validators over event-derived data,
- machine-checkable negative controls,
- same-seed normalized comparison,
- a human-readable report that tells agents what changed and where to look,
- lane integration that preserves existing artifact semantics.

Without this work, agents can still run many tests, but they mostly infer behavior from scattered
test names, source reading, raw logs, and substring assertions. The target state is a deterministic
simulation lane that produces a compact, trusted evidence bundle.

---

## 2. Reference standard (study these before writing code)

You must read the cited files directly before implementation. Do not rely on this summary alone.
The point is to adapt the evidence and validation patterns idiomatically to this Rust workspace,
not to copy another repository's architecture.

### 2.1 Current harness contracts

- `docs/testing.md` - canonical lane map, artifact layout, deletion policy, invariant owner table.
- `scripts/test-lanes.sh` - canonical lane runner and dry-run artifact shape.
- `scripts/stress-harness.sh` - existing stress runner, stage artifact writer, and prompt/run `events.jsonl` handling.
- `docs/architecture.md` - event-sourced runtime, replay purity, coordinator boundaries.
- `crates/harness-core/src/event.rs` - event vocabulary and schema owner.
- `crates/harness-core/src/store.rs` - event store and `events.jsonl` mechanics.
- `crates/harness/src/scenarios.rs` - current deterministic scenarios and mock provider setup.
- `crates/harness/src/run.rs` - scenario execution path.
- `crates/harness-providers/tests/recorded/*` - existing recorded provider tests.
- `crates/harness-testkit/src/secret_scanner.rs` and `crates/harness-testkit/tests/secretscan_test.rs` - secret hygiene surface.
- `crates/harness-testkit/tests/AGENTS.md` - T5 PTY/live/native signoff boundaries.

### 2.2 Inspiration repositories

- `inspirations/pi_agent_rust/docs/e2e_scenario_matrix.json` - scenario traceability matrix,
  required artifacts, replay commands, and live-policy fields.
- `inspirations/pi_agent_rust/docs/swarm-flight-recorder.md` - deterministic JSONL flight
  recorder with schema version, monotonic sequence validation, required identity fields,
  redaction summary, replay command, and summary report.
- `inspirations/pi_agent_rust/tests/e2e_provider_failure_injection.rs` - deterministic provider
  failure injection and JSONL artifact checks.
- `inspirations/pi_agent_rust/tests/e2e_golden_corpus.rs` - self-contained fixture rows with CLI
  args, stdin, cassettes, temp files, expected outcomes, and isolated env.
- `recorded-runner.ts` under the TypeScript LLM inspiration tree - recorded-case naming, filters,
  replay default, record mode, and missing-cassette behavior.

### 2.3 Transferable doctrine

1. Simulation evidence must be deterministic by construction, not by hope.
2. A matrix is useful only if validators enforce it.
3. JSONL is useful only if it is schema-versioned, redacted, and summarized.
4. Reports are useful only if agents can quickly see behavior deltas and failure causes.
5. Existing lane/stress/cassette/replay infrastructure is the default foundation.
6. Real PTY/live/native lanes remain small, opt-in, and provenance-focused.
7. Negative controls prove the validators are real.

---

## 3. Goals and non-goals

### 3.1 Goals

- [x] G1. Add one offline deterministic simulation lane reachable through `scripts/test-lanes.sh`.
- [x] G2. Make scenario and invariant ownership explicit through a checked-in matrix.
- [x] G3. Produce stable, normalized artifacts that agents can diff and inspect.
- [x] G4. Validate behavior semantically using event-derived predicates, not file-existence or
  substring-only checks.
- [x] G5. Prove same-seed stability by running the simulation twice, normalizing volatile fields,
  and comparing normalized summaries exactly.
- [x] G6. Generate an agent-readable report with narrative summary, behavior delta, invariant
  results, artifact index, replay commands, failure signals, and redaction summary.
- [x] G7. Extend or compose existing lane/stress/replay/cassette/secret-scan surfaces instead of
  replacing them.
- [x] G8. Add negative controls for every new validator family.
- [x] G9. Resolve the stale `crates/harness-testkit/AGENTS.md` `src/workflow_simulator.rs` reference.
- [x] G10. Update `docs/testing.md` only after the simulation lane and artifacts exist.

### 3.2 Non-goals

- N1. Building a live-provider evaluation suite.
- N2. Building a second harness runtime or parallel coordinator.
- N3. Replacing `scripts/test-lanes.sh` as the canonical lane runner.
- N4. Replacing `scripts/stress-harness.sh` without a written implementation comparison.
- N5. Replacing existing provider cassette support without proving a gap.
- N6. Making PTY/live/native/screenshot lanes behavioral invariant owners in MVP.
- N7. Requiring exact raw JSONL equality across same-seed runs.
- N8. Copying inspiration implementations literally.
- N9. Building a full golden-corpus product in MVP.
- N10. Adding new dependencies without explicit justification and comparison to existing workspace
  capabilities.

---

## 4. Principles -> Rust mechanisms

| Principle | Rust/workspace mechanism |
|---|---|
| Existing evidence lanes remain canonical | Compose through `scripts/test-lanes.sh` and preserve `summary.txt`, `env.txt`, and per-stage evidence. |
| Simulation ownership is explicit | Add `docs/simulation-matrix.json` with scenario rows, invariant IDs, determinism classes, artifacts, and replay commands. |
| Semantic behavior beats string matching | Write Rust validators for matrix rows, event rows, invariant ownership, replay predicates, tool/permission lifecycle, and same-seed normalized summaries. |
| Determinism is measured | Run same seed twice, normalize volatile fields, compare summaries exactly, and report actionable diffs. |
| JSONL is evidence, not the product | Generate `simulation-report.json` and `simulation-summary.txt` for human/agent triage. |
| Secrets never land in artifacts | Reuse or extend `harness-testkit` secret scanning for simulation artifact roots. |
| Signoff lanes stay signoff lanes | Matrix validator rejects behavioral invariant ownership by `pty-signoff`, `live-signoff`, `native-signoff`, `planned`, or `waived` rows. |
| Infrastructure is tested | Add negative-control fixtures/tests for each validator family. |

---

## 5. Simulation taxonomy (the only determinism classes)

Every scenario row must use exactly one of these `determinism_class` values:

- [x] `offline-deterministic` - may own behavioral invariants in MVP.
- [x] `pty-signoff` - provenance/signoff only; must not own behavioral invariants in MVP.
- [x] `live-signoff` - provenance/signoff only; must not own behavioral invariants in MVP.
- [x] `native-signoff` - provenance/signoff only; must not own behavioral invariants in MVP.
- [x] `planned` - no behavioral ownership.
- [x] `waived` - no behavioral ownership; must include a reason in the matrix.

Anything that does not fit these classes does not get added to the matrix.

---

## 6. Product requirements

### 6.1 MVP end state

- [x] 6.1.1. `scripts/test-lanes.sh simulation` or an explicitly documented equivalent command
  runs the simulation lane offline.
- [x] 6.1.2. The lane requires no live provider credentials and no real network access.
- [x] 6.1.3. The lane creates an artifact root following existing lane conventions.
- [x] 6.1.4. The lane reads a checked-in scenario matrix.
- [x] 6.1.5. The lane validates matrix rows, simulation events, invariant ownership, redaction,
  replay metadata, expected artifacts, and same-seed normalized stability.
- [x] 6.1.6. The lane fails on negative controls.
- [x] 6.1.7. The lane uses real harness events, replay/projection output, or a documented
  deterministic testkit source. Synthetic fake JSONL that bypasses runtime behavior is forbidden.
- [x] 6.1.8. The lane generates an agent-readable summary and JSON report.
- [x] 6.1.9. `docs/testing.md` documents the implemented command, artifacts, schemas, and failure
  semantics.

### 6.2 Required public surfaces

- [x] 6.2.1. `scripts/test-lanes.sh` includes the simulation lane and `--help`/dry-run behavior.
- [x] 6.2.2. `docs/testing.md` includes the simulation lane once implemented.
- [x] 6.2.3. A checked-in scenario matrix exists, recommended path `docs/simulation-matrix.json`.
- [x] 6.2.4. Schema versions are defined for matrix, event, report, artifact index, and
  normalization profile.
- [x] 6.2.5. Simulation artifacts are generated under the lane artifact root.
- [x] 6.2.6. Negative-control tests or generated fixtures exist.
- [x] 6.2.7. `crates/harness-testkit/AGENTS.md` no longer points to an absent simulator helper
  without qualification.

---

## 7. Artifact and schema contracts

### 7.1 Existing lane artifact shape to preserve

Every simulation run must include the existing lane-level files:

- [x] `summary.txt`
- [x] `env.txt`

Every simulation stage must include:

- [x] `command.txt`
- [x] `stdout.txt`
- [x] `stderr.txt`
- [x] `status.txt`
- [x] `verification.txt`

### 7.2 New simulation artifacts

The simulation stage must additionally include:

- [x] `simulation-matrix.json`
- [x] `simulation-events.jsonl`
- [x] `simulation-report.json`
- [x] `artifact-index.jsonl`, unless an equivalent existing index is reused and documented
- [x] `simulation-summary.txt` or a clearly marked simulation section appended to `summary.txt`

### 7.3 `simulation-events.jsonl` row shape

Each row must include at least:

```json
{
  "schema_version": "simulation-event-v1",
  "seq": 1,
  "scenario_id": "golden_path",
  "seed": "fixed-or-numeric-seed",
  "run_id": "stable-run-id-or-null",
  "run_fingerprint": "stable-fingerprint",
  "actor": "coordinator-or-provider-or-tool-or-replay",
  "component": "harness-core-or-harness-providers-or-harness-tools-or-harness",
  "event_kind": "event-or-derived-predicate-kind",
  "invariant_ids": ["INV-001"],
  "redaction": {
    "status": "clean",
    "redacted_fields": [],
    "scanner": "harness-testkit-secret-scanner"
  },
  "replay_command_fingerprint": "stable-fingerprint-or-null",
  "payload": {}
}
```

Rules:

- [x] `schema_version` is required.
- [x] `seq` is required and monotonic within a run.
- [x] `scenario_id` is required and known by the matrix.
- [x] `seed` is required.
- [x] `run_id` or `run_fingerprint` is required.
- [x] `actor` is required.
- [x] `component` is required.
- [x] `event_kind` is required.
- [x] `invariant_ids` are required when the row contributes to an invariant.
- [x] `redaction` is required.
- [x] `replay_command_fingerprint` is required and may be null only when replay is not applicable.
- [x] `payload` is redacted or contains derived predicate data only.

### 7.4 `simulation-report.json` shape

`simulation-report.json` must include at least:

```json
{
  "schema_version": "simulation-report-v1",
  "matrix_schema_version": "simulation-matrix-v1",
  "event_schema_version": "simulation-event-v1",
  "run": {
    "seed": "fixed-or-numeric-seed",
    "run_fingerprint": "stable-fingerprint",
    "normalization_profile": "simulation-normalization-v1"
  },
  "summary": {
    "status": "pass-or-fail",
    "narrative": "short human-readable summary"
  },
  "behavior_delta": [],
  "invariant_results": [],
  "artifact_index": [],
  "replay_commands": [],
  "failure_signals": [],
  "redaction_summary": {},
  "volatile_fields": [],
  "raw_evidence_paths": []
}
```

Required report sections:

- [x] narrative summary
- [x] behavior delta
- [x] invariant pass/fail table
- [x] artifact index
- [x] replay commands
- [x] top contributing failure signals
- [x] redaction summary
- [x] paths to raw evidence
- [x] volatile field declarations

### 7.5 `simulation-summary.txt` content

The text summary must be stable and agent-readable. It must include:

- [x] simulation status
- [x] seed
- [x] run fingerprint
- [x] matrix version
- [x] event schema version
- [x] scenario count by `determinism_class`
- [x] invariant pass/fail table
- [x] negative-control pass/fail table
- [x] behavior delta summary
- [x] replay command list
- [x] artifact index path
- [x] redaction status
- [x] top failure signals
- [x] same-seed comparison status

The summary must not include unnormalized absolute paths, timestamps, temp directories, hostnames,
process IDs, or unstable UUIDs unless those values are explicitly declared volatile and excluded
from same-seed comparison.

### 7.6 `artifact-index.jsonl` row shape

Each row must include at least:

```json
{
  "schema_version": "artifact-index-v1",
  "scenario_id": "golden_path",
  "artifact_kind": "stdout-or-events-or-report-or-replay-or-cassette",
  "path": "relative/path/from/artifact/root",
  "redaction_status": "clean",
  "producer": "script-or-test-or-validator-name",
  "fingerprint": "stable-content-fingerprint"
}
```

Rules:

- [x] Paths are relative to the artifact root.
- [x] Fingerprints are stable after normalization.
- [x] Secret-bearing artifacts cannot be indexed as clean.

---

## 8. Scenario matrix contract

### 8.1 Required file and top-level shape

- [x] 8.1.1. A checked-in scenario matrix exists. Recommended path: `docs/simulation-matrix.json`.
- [x] 8.1.2. If another path is chosen, `docs/testing.md`, validators, and lane output name it
  explicitly.
- [x] 8.1.3. The top-level shape is this or an equivalent strictly documented shape:

```json
{
  "schema_version": "simulation-matrix-v1",
  "invariants": [],
  "scenarios": []
}
```

### 8.2 Required scenario fields

Each scenario row must include:

- [x] `scenario_id`
- [x] `description`
- [x] `determinism_class`
- [x] `invariant_ids`
- [x] `owner_tests_or_lanes`
- [x] `replay_command`
- [x] `expected_artifacts`
- [x] `seed_policy`
- [x] `artifact_schema_versions`
- [x] `negative_controls`
- [x] `live_policy` when applicable

Optional but recommended fields:

- [x] `source_surface`
- [x] `fixture_paths`
- [x] `provider_mode`
- [x] `failure_modes`
- [x] `report_sections`

### 8.3 Example row

```json
{
  "scenario_id": "golden_path",
  "description": "Deterministic golden path run using mock provider",
  "determinism_class": "offline-deterministic",
  "invariant_ids": ["INV-001"],
  "owner_tests_or_lanes": ["scripts/test-lanes.sh simulation"],
  "replay_command": "cargo run -p harness -- replay ...",
  "expected_artifacts": [
    "simulation-events.jsonl",
    "simulation-report.json"
  ],
  "seed_policy": "fixed seed recorded in artifacts",
  "artifact_schema_versions": {
    "matrix": "simulation-matrix-v1",
    "event": "simulation-event-v1",
    "report": "simulation-report-v1"
  },
  "negative_controls": ["unknown-invariant-id-fails"],
  "live_policy": "not-applicable",
  "source_surface": "crates/harness/src/scenarios.rs",
  "fixture_paths": [],
  "provider_mode": "mock-or-recorded-cassette",
  "failure_modes": [],
  "report_sections": []
}
```

### 8.4 Matrix validation failures

The matrix validator must fail for:

- [x] missing required field
- [x] unknown `determinism_class`
- [x] duplicate `scenario_id`
- [x] unknown invariant ID
- [x] invariant with no owning scenario
- [x] scenario owning behavioral invariants while not `offline-deterministic`
- [x] missing expected artifact declaration
- [x] missing or malformed schema version declaration

---

## 9. Validators and negative controls

### 9.1 Required validators

- [x] 9.1.1. Matrix schema validity.
- [x] 9.1.2. Scenario ID uniqueness.
- [x] 9.1.3. Known invariant IDs.
- [x] 9.1.4. Valid `determinism_class`.
- [x] 9.1.5. Behavioral invariant ownership restricted to `offline-deterministic`.
- [x] 9.1.6. Expected artifacts exist and are indexed.
- [x] 9.1.7. `simulation-events.jsonl` schema validity.
- [x] 9.1.8. Monotonic event sequence numbers.
- [x] 9.1.9. Required actor identity.
- [x] 9.1.10. Required component identity.
- [x] 9.1.11. Scenario ID exists in matrix.
- [x] 9.1.12. Invariant IDs exist in matrix.
- [x] 9.1.13. Permission/tool-call lifecycle where applicable.
- [x] 9.1.14. Replay/projection consistency where applicable.
- [x] 9.1.15. Provider/cassette determinism where provider flows are included.
- [x] 9.1.16. Redaction summary exists for each event row.
- [x] 9.1.17. Secret scanner passes over the simulation artifact root.
- [x] 9.1.18. Same-seed normalized summary stability.

### 9.2 Required negative controls

Negative controls must be checked in or generated by tests. Prose-only claims do not satisfy this
section. The suite must fail deterministically for:

- [x] matrix drift
- [x] unknown invariant ID
- [x] invalid schema row
- [x] duplicate scenario ID
- [x] non-monotonic JSONL sequence
- [x] missing actor identity
- [x] missing component identity
- [x] unknown scenario ID in event row
- [x] signoff row claiming behavioral ownership
- [x] missing expected artifact
- [x] same-seed normalized summary mismatch
- [x] secret-bearing artifact
- [x] missing cassette when provider cassette work is in MVP
- [x] request mismatch when provider cassette work is in MVP
- [x] secret-bearing cassette when provider cassette work is in MVP

Each failure must name:

- [x] failing control
- [x] file or artifact path
- [x] scenario ID when applicable
- [x] invariant ID when applicable
- [x] expected value
- [x] observed value

---

## 10. Logging and observability requirements

The simulation lane must provide deterministic observability, not merely more logs.

### 10.1 Required outputs

- [x] `simulation-report.json`
- [x] `simulation-summary.txt` or simulation section in `summary.txt`
- [x] `artifact-index.jsonl`
- [x] validator stdout/stderr captured in stage artifacts
- [x] failure signal list
- [x] redaction summary
- [x] replay command list
- [x] behavior delta section

### 10.2 Behavior delta

The behavior delta section must compare the current run against expected normalized predicates for
each scenario. It must report:

- [x] added predicate
- [x] removed predicate
- [x] changed predicate
- [x] changed artifact fingerprint
- [x] changed replay result
- [x] changed provider request digest where applicable

### 10.3 Failure signals

The report must identify top contributing failure signals using only these categories unless the
schema is intentionally revised:

- [x] `matrix-schema`
- [x] `event-schema`
- [x] `sequence`
- [x] `identity`
- [x] `invariant`
- [x] `permission-lifecycle`
- [x] `tool-lifecycle`
- [x] `replay`
- [x] `provider-cassette`
- [x] `redaction`
- [x] `secret-scan`
- [x] `same-seed-stability`
- [x] `artifact-missing`
- [x] `normalization`

### 10.4 Redaction summary

The redaction summary must include:

- [x] scanner used
- [x] scanned artifact count
- [x] clean artifact count
- [x] redacted field count
- [x] rejected artifact count
- [x] secret finding count
- [x] paths to rejected artifacts when applicable

### 10.5 Replay command observability

Each scenario row must name a replay command. Each report must include:

- [x] exact replay command or documented placeholder if not applicable
- [x] replay command fingerprint
- [x] replay validation status
- [x] path to replay evidence when applicable

---

## 11. Implementation plan by area

### 11.1 Matrix and invariant contract

- [x] 11.1.1. Add the checked-in matrix.
- [x] 11.1.2. Add invariant definitions.
- [x] 11.1.3. Add invalid matrix fixtures.
- [x] 11.1.4. Add tests before implementation for missing fields, duplicate IDs, unknown invariant
  IDs, and signoff behavioral ownership.
- [x] 11.1.5. Admit only existing surfaces at first, especially `golden_path` and any directly
  justified `offline-deterministic` scenario from `crates/harness/src/scenarios.rs`.

### 11.2 Schema and event validators

- [x] 11.2.1. Define `simulation-event-v1`.
- [x] 11.2.2. Define `simulation-report-v1`.
- [x] 11.2.3. Define `artifact-index-v1`.
- [x] 11.2.4. Define `simulation-normalization-v1`.
- [x] 11.2.5. Add tests for JSONL sequence, identity, scenario mapping, invariant mapping,
  redaction fields, and actionability of error messages.

### 11.3 Runner composition

- [x] 11.3.1. Add the simulation lane to `scripts/test-lanes.sh`.
- [x] 11.3.2. Preserve dry-run behavior and artifact shape.
- [x] 11.3.3. Capture command/stdout/stderr/status/verification for each simulation stage.
- [x] 11.3.4. Generate required simulation artifacts.
- [x] 11.3.5. Run same-seed normalized comparison automatically.
- [x] 11.3.6. Fail with artifact paths and validator output when any check fails.

### 11.4 Scenario admission and semantic invariants

- [x] 11.4.1. Map `golden_path` to matrix rows and invariant IDs.
- [x] 11.4.2. Derive simulation rows from real harness events, replay/projection output, or a
  documented deterministic testkit source.
- [x] 11.4.3. Add replay/projection consistency validators where applicable.
- [x] 11.4.4. Add permission/tool lifecycle validators where applicable.
- [x] 11.4.5. Add provider/cassette validators only if provider cassette scenarios are admitted
  into MVP.

### 11.5 Secret scanning

- [x] 11.5.1. Include simulation artifact roots in secret scanning.
- [x] 11.5.2. Reuse `HARNESS_SECRETS_SCAN_ARTIFACTS` behavior or document and test an extension.
- [x] 11.5.3. Cover simulation JSONL, reports, summaries, artifact index, and cassettes when in
  scope.
- [x] 11.5.4. Add a secret-bearing simulation artifact negative control.

### 11.6 Docs and stale guidance resolution

- [x] 11.6.1. Update `docs/testing.md` after the lane exists.
- [x] 11.6.2. Document the command, artifacts, schemas, same-seed comparison, and failure modes.
- [x] 11.6.3. Resolve `crates/harness-testkit/AGENTS.md` stale `src/workflow_simulator.rs` reference
  by implementing the helper, retargeting the guidance, or removing the stale reference.
- [x] 11.6.4. Ensure `docs/testing.md` and `scripts/test-lanes.sh --help` agree exactly.

### 11.7 Implementation comparison notes

- [x] 11.7.1. Record a comparison note for every new helper, runner, framework, fixture corpus,
  recorder, or simulator abstraction.
- [x] 11.7.2. Each note must name the existing surface inspected, the gap found, and the chosen
  reuse or extension strategy.
- [x] 11.7.3. Notes may live in this PRD's progress appendix, a dedicated progress doc, or code
  comments close to the new helper when the note is short and architectural.

---

## 12. Acceptance gates (machine-checkable, run and keep output)

Each gate has an exact command or required command family. Run it, capture real output, and record
pass/fail with artifact paths. If a tool is unavailable, document the blocker and run the closest
repository-approved substitute; do not silently skip the gate.

- [x] **A1 - Matrix validator green.** Matrix validator tests pass and invalid fixtures fail with
  actionable errors.
- [x] **A2 - Event validator green.** Event/report/artifact-index validator tests pass and invalid
  JSONL controls fail.
- [x] **A3 - Simulation lane dry-run.** `scripts/test-lanes.sh simulation --dry-run` writes the
  expected artifact shape without running underlying commands.
- [x] **A4 - Simulation lane real run.** `scripts/test-lanes.sh simulation` exits zero and writes
  all required lane and simulation artifacts.
- [x] **A5 - Same-seed stability.** The simulation lane runs twice with the same seed, normalizes
  volatile fields, compares normalized summaries exactly, and records PASS.
- [x] **A6 - Same-seed negative control.** A controlled normalized-summary mismatch fails with an
  actionable diff naming paths/keys.
- [x] **A7 - Signoff ownership guard.** A matrix row with `pty-signoff`, `live-signoff`,
  `native-signoff`, `planned`, or `waived` claiming behavioral invariant ownership fails.
- [x] **A8 - Secret scan.** The simulation artifact root is scanned, passes when clean, and fails
  on a secret-bearing artifact control.
- [x] **A9 - Replay/projection consistency.** Any admitted replay scenario records replay command
  fingerprints and validator status.
- [x] **A10 - Provider cassette determinism.** If provider cassette scenarios are admitted, missing
  cassette/request mismatch/secret cassette controls fail. If not admitted, this is explicitly
  marked post-MVP with no checkbox claim.
- [x] **A11 - Artifact index.** Every required artifact is present in `artifact-index.jsonl` or the
  documented equivalent, with relative paths and stable fingerprints.
- [x] **A12 - Docs current.** `docs/testing.md`, `scripts/test-lanes.sh --help`, and any relevant
  `AGENTS.md` guidance describe only implemented behavior.
- [x] **A13 - Stale guidance resolved.** `crates/harness-testkit/AGENTS.md` no longer contains an
  unqualified stale reference to absent `src/workflow_simulator.rs`.
- [x] **A14 - Standard repository checks.** Run:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test -p harness-testkit
cargo test -p harness-core
cargo test -p harness-providers
scripts/test-lanes.sh fast
```

- [x] **A15 - Full deterministic closeout.** Run:

```bash
scripts/test-lanes.sh all-deterministic
```

Recommended final checks:

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

---

## 13. Definition of Done (you may not stop before all are true)

- [x] DoD-1. Every checkbox in Sections 3, 6, 7, 8, 9, 10, and 11 is complete, or explicitly
  marked Post-MVP with a reason.
- [x] DoD-2. Every acceptance gate A1-A15 has fresh evidence from this implementation pass.
- [x] DoD-3. The simulation lane runs offline with no live credentials and no real network.
- [x] DoD-4. Same-seed normalized summaries compare exactly.
- [x] DoD-5. Negative controls prove validators fail when they should.
- [x] DoD-6. Reports are agent-readable and include behavior deltas and failure signals.
- [x] DoD-7. No PTY/live/native/signoff row owns behavioral invariants.
- [x] DoD-8. Secret scanning covers the simulation artifact root.
- [x] DoD-9. The stale `workflow_simulator.rs` guidance is resolved.
- [x] DoD-10. `docs/testing.md` and `scripts/test-lanes.sh` agree.
- [x] DoD-11. Runtime invariants remain intact: coordinator authority, replay purity, event source
  of truth, permission checks before tool execution, and side-effect-free replay.

If any item is false, the work is not done. Continue or write an honest checkpoint; do not declare
completion.

---

## 14. Anti-gaming rules

- [x] 14.1. No artifact-only success. Validators must inspect content and semantics.
- [x] 14.2. No substring-only semantic validation.
- [x] 14.3. No synthetic fake JSONL that bypasses runtime behavior while claiming runtime proof.
- [x] 14.4. No raw JSONL equality as the same-seed gate.
- [x] 14.5. No unnormalized absolute paths, timestamps, temp dirs, hostnames, process IDs, or
  irrelevant UUIDs in normalized summaries.
- [x] 14.6. No new runner/helper/recorder/framework without an implementation comparison note.
- [x] 14.7. No documentation of behavior before the behavior exists.
- [x] 14.8. No widening live/PTY/native lanes into behavioral owners.
- [x] 14.9. No weakening existing tests or deleting coverage to make the simulation lane easier.
- [x] 14.10. No marking provider cassette determinism complete unless the required cassette
  negative controls pass or are explicitly deferred as Post-MVP.
- [x] 14.11. No secrets in artifacts, reports, summaries, cassettes, or indexes.
- [x] 14.12. No stopping on self-report. Re-derive evidence with commands.

---

## 15. Suggested execution order (phases with exit criteria)

You may resequence within a phase, but you may not claim a phase complete until its exit criteria
hold.

- [x] **Phase 0 - Baseline and comparison notes.** Re-read the current lane, stress, scenario,
  replay, cassette, and secret-scan surfaces. Record implementation comparison notes for any new
  abstraction you plan to add. *Exit:* comparison notes exist and no new helper is unaccounted for.
- [x] **Phase 1 - Matrix.** Add the scenario/invariant matrix and matrix validator tests with
  negative controls. *Exit:* invalid matrix controls fail and valid matrix passes.
- [x] **Phase 2 - Schemas and validators.** Add event/report/artifact-index schemas and validators.
  *Exit:* malformed events, non-monotonic sequences, missing identity, unknown scenarios, and unknown
  invariants fail.
- [x] **Phase 3 - Runner integration.** Compose the lane into `scripts/test-lanes.sh` and preserve
  artifact shape. *Exit:* dry-run and real simulation run produce expected artifacts.
- [x] **Phase 4 - Same-seed normalization.** Add normalization and exact normalized-summary
  comparison. *Exit:* same-seed pass and mismatch negative control both work.
- [x] **Phase 5 - Scenario admission.** Admit existing deterministic scenarios and semantic
  invariants. *Exit:* every admitted scenario maps to matrix rows and validators.
- [x] **Phase 6 - Secret scan and provider/replay hardening.** Integrate artifact-root secret
  scanning and any in-scope provider/replay validators. *Exit:* negative controls pass/fail as
  required.
- [x] **Phase 7 - Docs and stale guidance.** Update `docs/testing.md` and resolve
  `crates/harness-testkit/AGENTS.md`. *Exit:* docs describe implemented behavior only.
- [x] **Phase 8 - Closeout.** Run all acceptance gates and record evidence. *Exit:* Section 13 is
  fully true.

---

## 16. Task dependency graph

| Task | Depends on | Reason |
|---|---|---|
| Matrix and invariant contract | None | Establishes ownership source of truth. |
| Matrix validator and negative controls | Matrix contract | Needs schema and invariant IDs. |
| Event/report/artifact schemas | Matrix contract | Must reference matrix IDs and schema versions. |
| Event validator and negative controls | Event/report/artifact schemas | Needs event schema. |
| Lane runner composition | Matrix and event validators | Must run validators and preserve artifacts. |
| Same-seed normalization | Lane runner composition | Needs runner artifacts to normalize and compare. |
| Scenario admission | Matrix and runner | Needs matrix and runner before adding scenario evidence. |
| Replay/tool/provider validators | Scenario admission | Needs admitted scenario surfaces. |
| Secret-scan integration | Lane runner composition | Needs artifact root shape. |
| Docs and stale guidance resolution | All public contracts above | Must document implemented behavior only. |
| Final QA | All previous tasks | Verifies integrated behavior. |

Parallel waves:

1. Matrix contract and schema spike.
2. Matrix validator and event validator.
3. Lane runner composition and secret-scan integration.
4. Same-seed normalization and scenario admission.
5. Replay/tool/provider validators and docs.
6. Final QA.

Critical path:

```text
Matrix -> Matrix validator -> Lane runner -> Same-seed normalization -> Scenario admission -> Semantic validators -> Docs -> Final QA
```

---

## 17. Atomic commit strategy

Use small, independently reviewable commits.

1. Matrix contract and validator tests.
2. Event/report/artifact schemas and validator tests.
3. Simulation lane runner composition.
4. Same-seed normalization and summary comparison.
5. Scenario admission and semantic validators.
6. Secret-scan integration and negative controls.
7. Documentation and stale guidance resolution.
8. Final QA cleanup only if needed.

Do not mix unrelated refactors into these commits.

---

## 18. Post-MVP hardening

These items are out of scope for MVP unless explicitly requested after MVP acceptance:

- Full golden-corpus product.
- Unused-interaction cassette checks if not already supported.
- PTY/native/live artifact parity with simulation artifacts.
- Live budget policy enforcement beyond matrix metadata.
- Expanded provider failure injection beyond admitted MVP scenarios.
- Rich latency trend dashboards.
- Cross-run historical comparisons beyond same-seed stability.
- New cassette recorder UX.
- New scenario authoring DSL.
- Broad visual report generation.

---

## 19. Reference index (open these)

Current repository:

- `AGENTS.md`
- `crates/harness/AGENTS.md`
- `crates/harness-core/AGENTS.md`
- `crates/harness-testkit/AGENTS.md`
- `crates/harness-testkit/tests/AGENTS.md`
- `docs/architecture.md`
- `docs/testing.md`
- `docs/test-suite-prd.md`
- `scripts/test-lanes.sh`
- `scripts/stress-harness.sh`
- `crates/harness/src/scenarios.rs`
- `crates/harness/src/run.rs`
- `crates/harness-core/src/event.rs`
- `crates/harness-core/src/store.rs`
- `crates/harness-providers/tests/recorded/*`
- `crates/harness-testkit/src/secret_scanner.rs`
- `crates/harness-testkit/tests/secretscan_test.rs`

Inspiration repositories:

- `inspirations/pi_agent_rust/docs/e2e_scenario_matrix.json`
- `inspirations/pi_agent_rust/docs/swarm-flight-recorder.md`
- `inspirations/pi_agent_rust/tests/e2e_provider_failure_injection.rs`
- `inspirations/pi_agent_rust/tests/e2e_golden_corpus.rs`
- `recorded-runner.ts` under the TypeScript LLM inspiration tree

---

## 20. Implementation closeout evidence (2026-05-26)

### 20.1 Scope resolution

- MVP admitted scenario: `golden_path` only, via the existing `harness run --scenario golden_path --deterministic` surface and replay JSON.
- Provider cassette determinism is explicitly Post-MVP for this implementation because the admitted scenario uses the mock provider surface, not recorded provider cassettes. The matrix records this under `post_mvp` for `provider-cassette-determinism`.
- No PTY, live-provider, or native-signoff row owns behavioral invariants; the checked-in matrix admits only `offline-deterministic` behavioral ownership.
- New helper comparison: `crates/harness-testkit/src/simulation.rs` and `src/bin/simulation_evidence.rs` reuse `scripts/test-lanes.sh`, real harness scenario runs, replay JSON, and the existing secret scanner instead of adding a separate runner, cassette recorder, or workflow simulator.

### 20.2 Implemented surfaces

- `docs/simulation-matrix.json` defines `simulation-matrix-v1`, invariant IDs, `golden_path`, schema versions, required artifacts, negative controls, expected predicates, and the cassette Post-MVP note.
- `scripts/test-lanes.sh simulation` runs matrix/negative-control tests, two deterministic `golden_path` runs, replay for both runs, evidence generation, and simulation artifact secret scanning.
- `scripts/test-lanes.sh all-deterministic` includes `simulation` before `fast` and `integration`.
- `crates/harness-testkit/src/simulation.rs` validates the matrix, simulation event rows, report shape, artifact index rows and fingerprints, redaction, same-seed normalized summaries, and semantic invariants.
- `crates/harness-testkit/src/bin/simulation_evidence.rs` generates `simulation-matrix.json`, `simulation-events.jsonl`, `simulation-report.json`, `artifact-index.jsonl`, `simulation-summary.txt`, normalized summaries, same-seed comparison, and raw evidence copies.
- `crates/harness-testkit/tests/simulation_validator_test.rs` covers required negative controls: matrix drift, unknown invariant, invalid schema row, duplicate scenario ID, signoff behavioral ownership, missing expected artifact, fingerprint mismatch, non-monotonic sequence, missing actor, missing component, unknown scenario in event rows, unknown invariant in event rows, same-seed mismatch, and secret-bearing artifact.
- `crates/harness-testkit/tests/secretscan_test.rs` accepts `HARNESS_SIMULATION_ARTIFACT_DIR` when artifact scanning is enabled.
- `docs/testing.md` and `scripts/test-lanes.sh --help` document the implemented simulation lane, artifacts, schema versions, same-seed semantics, cassette Post-MVP boundary, and provenance-only signoff lanes.
- `crates/harness-testkit/AGENTS.md` now points to the implemented simulation validators instead of stale `src/workflow_simulator.rs` guidance.

### 20.3 Acceptance gate evidence

Fresh closeout commands for this implementation pass:

| Gate | Evidence |
|---|---|
| A1 Matrix validator green | `cargo test -p harness-testkit --test simulation_validator_test` passed with 26 tests. |
| A2 Event/report/index validator green | Same validator suite plus `scripts/test-lanes.sh simulation` passed; malformed event/index controls fail deterministically. |
| A3 Simulation dry-run | `scripts/test-lanes.sh simulation --dry-run --artifact-dir <artifact-root>/simulation-final-dry-run`. |
| A4 Simulation real run | `scripts/test-lanes.sh simulation --artifact-dir <artifact-root>/simulation-final`. |
| A5 Same-seed stability | `same-seed-comparison.txt` reports `status=pass`; normalized baseline/repeat summaries match exactly. |
| A6 Same-seed negative control | `same_seed_normalized_summary_mismatch_fails_with_path` reports the mismatching JSON path and values. |
| A7 Signoff ownership guard | `signoff_row_claiming_behavioral_ownership_fails` rejects non-offline behavioral ownership. |
| A8 Secret scan | Simulation lane runs `HARNESS_SECRETS_SCAN_ARTIFACTS=1 HARNESS_SIMULATION_ARTIFACT_DIR=<simulation-artifacts> cargo test -p harness-testkit --test secretscan_test`; `secret_bearing_artifact_is_rejected_by_scanner` is the negative control. |
| A9 Replay/projection consistency | `simulation-report.json` includes replay command fingerprint and `validation_status=pass`; `INV-003` passes against replay predicates. |
| A10 Provider cassette determinism | Post-MVP, recorded in `docs/simulation-matrix.json` because no cassette-backed scenario is admitted. |
| A11 Artifact index | `artifact-index.jsonl` includes every required artifact with relative paths, clean redaction status, and validated normalized fingerprints. |
| A12 Docs current | `docs/testing.md`, `scripts/test-lanes.sh --help`, and `crates/harness-testkit/AGENTS.md` describe the implemented lane only. |
| A13 Stale guidance resolved | `crates/harness-testkit/AGENTS.md` no longer contains an unqualified `src/workflow_simulator.rs` reference. |
| A14 Standard checks | `cargo fmt --all -- --check`, `cargo check --workspace`, `cargo test -p harness-testkit`, `cargo test -p harness-core`, `cargo test -p harness-providers`, and `scripts/test-lanes.sh fast`. |
| A15 Full deterministic closeout | `scripts/test-lanes.sh all-deterministic --artifact-dir <artifact-root>/all-deterministic-final`. |

Recommended final checks for this pass passed: `cargo clippy --workspace --all-targets --all-features -- -D warnings` and `cargo test --workspace --all-features`.

### 20.4 Definition of Done status

DoD-1 through DoD-11 are satisfied by the implemented surfaces and evidence above, except provider cassette determinism controls are intentionally Post-MVP until a cassette-backed scenario is admitted. Runtime invariants remain unchanged: simulation consumes real harness events and replay output, replay remains side-effect-free, and no coordinator authority or permission-check behavior moved into the testkit.

---

*End of PRD. Begin at Phase 0. Do not stop until Section 13 is fully satisfied with re-derived evidence.*

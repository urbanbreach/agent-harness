# Harness ↔ oh-my-codex parity dossier

**Status:** completed Harness-native contract for the registry-first parity refactor.  
**Audience:** maintainers verifying or extending the OMX command / workflow parity rewrite.  
**Scope:** visible `$<skill>` workflow commands, slash-native utilities, workflow protocol prompt bodies, active workflow overlays, native team/question handoff, and replayable proof boundaries.

This dossier is the closeout record for four contract surfaces:

1. prompt guidance contract
2. runtime overlays and proof boundaries
3. team orchestration
4. doctor / conformance / replay evidence

The accepted implementation is **Harness-native**:

- workflow state is derived from event-sourced projections, not per-mode JSON authority
- material progress is recorded as coordinator-owned evidence
- replay artifacts and deterministic projections are the proof boundary, not tmux substrate
- active workflow context is reinforced through recurring prompt overlays, not one-shot activation prose

Rejected proof substrates remain rejected: state-file authority, tmux-pane orchestration, hidden shell launchers, and compatibility HUD files are not lifecycle evidence for this harness.

Reference anchors:

- [`.omx/plans/prd-omx-parity-dossier-finish.md`](../.omx/plans/prd-omx-parity-dossier-finish.md)
- [`.omx/plans/test-spec-omx-parity-dossier-finish.md`](../.omx/plans/test-spec-omx-parity-dossier-finish.md)
- OMX prompt guidance contract: [`inspirations/oh-my-codex/docs/prompt-guidance-contract.md`](../inspirations/oh-my-codex/docs/prompt-guidance-contract.md)
- OMX native hooks mapping: [`inspirations/oh-my-codex/docs/codex-native-hooks.md`](../inspirations/oh-my-codex/docs/codex-native-hooks.md)
- OMX team orchestration prompts: [`inspirations/oh-my-codex/prompts/team-orchestrator.md`](../inspirations/oh-my-codex/prompts/team-orchestrator.md), [`team-executor.md`](../inspirations/oh-my-codex/prompts/team-executor.md)
- Doctor / conformance / replay references: [`inspirations/pi_agent_rust/docs/franken-node-compatibility-doctor-contract.json`](../inspirations/pi_agent_rust/docs/franken-node-compatibility-doctor-contract.json), [`conformance-operator-playbook.md`](../inspirations/pi_agent_rust/docs/conformance-operator-playbook.md)
- Replay examples: [`inspirations/pi_agent_rust/tests/franken_node_deterministic_replay_contract.rs`](../inspirations/pi_agent_rust/tests/franken_node_deterministic_replay_contract.rs), [`e2e_replay_bundles.rs`](../inspirations/pi_agent_rust/tests/e2e_replay_bundles.rs)

---

## 1. Prompt guidance contract

### Completed behavior

Harness workflow commands now resolve to typed workflow-skill dispatch rather than shell snippets. The shipped `SKILL.md` bodies are treated as runtime protocol inputs and are validated for Harness-native operating sections:

- `Purpose`
- `Use when`
- `Harness state contract`
- `Execution protocol`
- `Evidence and closeout contract`
- `Stop/escalation conditions`
- `Verification checklist`

High-traffic workflow families (`deep-interview`, `ralplan`/`plan`, `ralph`, `team`, `ultrawork`, `autopilot`, `ultraqa`, `ultragoal`, `ecomode`, `visual-*`, and `web-clone`) preserve the important behavioral gates while translating substrate-specific mechanics into coordinator evidence, projections, native question/team tools, and replayable closeout.

### Regression guard

`harness doctor --json` includes `workflow_skill_protocol_native`. The check inspects shipped workflow skill bodies for required protocol sections and fails closed on forbidden operational substrate wording. Findings are machine-readable by skill path, token, reason code, severity, and remediation.

---

## 2. Runtime overlays and proof boundaries

### Completed behavior

Active workflow context is projected from recorded events and prepended to later live provider turns as a bounded `<system-reminder>` block. The overlay includes deterministic workflow ordering, mode/phase/status/owner, active continuation information, protocol hints, and an explicit rule not to start or cancel workflows without user intent.

The overlay uses `WorkflowProjection` as the authority. It does not read compatibility state files and does not infer lifecycle state from an external terminal runtime.

### Deterministic ordering

Active workflows are sorted by a fixed orchestration priority before lexicographic and workflow-id tie breakers:

`deep-interview → ralplan → team → autopilot → ralph → ultrawork → ultraqa → ultragoal → autoresearch → ecomode → visual → web-clone → pipeline → unknown`

This keeps planning/interview/team context visible before execution-loop context when multiple compatible workflows are active.

---

## 3. Team orchestration

### Completed behavior

`$team` is the user-facing workflow contract. Staffing tokens such as `2:executor 1:verifier` are parsed as native planning context and appear in workflow prompt metadata, not as a shell launch recipe.

The team prompt and handoff point at native coordinator tools:

- `team_create`
- `team_task_create`
- `team_send_message`
- `team_list`

The broader native surface also includes status, task listing/get/update, and shutdown approve/reject tools. Team lifecycle proof is coordinator-owned team/task/message/shutdown evidence.

### Preserved principles

The parity rewrite keeps conservative fanout, leader-owned integration, bounded task ownership, worker mailbox discipline, and verification before shutdown. It translates or deletes tmux-specific launch, pane recovery, and environment-variable mechanics from user-facing skill protocols.

---

## 4. Doctor / conformance / replay evidence

### Completed proof surfaces

Parity proof now comes from first-party checks and replayable artifacts:

- command registry drift and workflow inventory checks
- workflow transition policy tests
- workflow closeout policy/readiness checks
- protocol-body doctor diagnostics
- active workflow overlay tests
- multi-skill/deferred handoff tests
- autopilot review loopback and closeout-gate tests
- config/docs drift tests and deterministic lane runner evidence

`harness doctor --json` reports native workflow readiness through stable check ids including:

- `workflow_contract_registry`
- `workflow_dollar_aliases`
- `workflow_skill_loadability`
- `workflow_skill_protocol_native`
- `workflow_transition_policy`
- `workflow_context_snapshot`
- `workflow_runtime_config`
- `workflow_closeout_policy`
- `workflow_closeout_readiness`

The doctor output distinguishes native implementation evidence from deprecated fallback substrate claims.

---

## Closeout evidence

Durable implementation evidence is recorded under `.omx/ultragoal/`:

- G011: ordered multi-skill/deferred parsing, native team staffing context, transition-policy handoff metadata, and per-turn active workflow overlays.
- G012: native skill protocol doctor/conformance diagnostics, autopilot review loopback/closeout audit coverage, this dossier closeout, and final verification gate.

Focused verification performed for this dossier includes:

```bash
cargo test -p harness-tui multi_skill -- --nocapture
cargo test -p harness-tui dollar_deep_interview_chain_records_ralplan_as_next_workflow -- --nocapture
cargo test -p harness-tui dollar_team_staffing_tokens_are_native_team_context -- --nocapture
cargo test -p harness-tui dollar_multi_skill_invalid_chain_fails_closed_with_draft_preserved -- --nocapture
cargo test -p harness-tui
cargo test -p harness native_workflow_protocol_detects_forbidden_substrate_fixture -- --nocapture
cargo test -p harness --test config_schema_cli doctor_cli_emits_json_report -- --nocapture
cargo test -p harness --test config_schema_cli doctor_cli_reports_shipped_orchestration_health -- --nocapture
cargo test -p harness-core review_verdict_clean_completes_active_autopilot_workflow -- --nocapture
cargo test -p harness-core review_verdict_non_clean_returns_autopilot_to_ralplan_phase -- --nocapture
cargo test -p harness-core closeout_policy_denies_autopilot_finish_until_review_gate_is_present -- --nocapture
```

Final closeout additionally requires the root formatting, workspace check, clippy, doctor/config validation, static forbidden-token scan, `scripts/test-lanes.sh fast`, the `ai-slop-cleaner` pass, and `$code-review` APPROVE/CLEAR gate captured by the ultragoal ledger.

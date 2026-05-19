# Harness OMX workflow inventory

The command-parity inventory is the checked matrix for Harness-native OMX command parity. It separates runtime command semantics from evidence metadata so the coordinator, CLI, TUI, docs, and tests can drift-check the same command contract without making the TUI or documentation an alternate runtime authority.

Canonical fixture: `crates/harness/tests/fixtures/harness_omx_workflow_inventory.json`

Deterministic drift gate: `cargo test -p harness --test workflow_inventory`

## Locked reference counts

- 44 reference OMX workflow skills are inventoried from `inspirations/oh-my-codex/skills/*/SKILL.md`; example: `omx-skill:ultragoal`.
- 30 slash-agent roles are inventoried from the approved command-parity consensus plan; example: `slash-agent:executor`.
- Harness workflow registry commands such as `workflow-run`, `workflow-evidence`, `plan-consensus`, and `goal-ledger` must have inventory rows before they can be surfaced as present.

## Row expectations

Every row records:

- `canonical_id`
- `aliases`
- `source_refs`
- `harness_mapping`
- `status`
- `visibility`
- `authority_model`
- `blocker_or_stage`
- `tests`
- optional `runtime_semantics` for registry-backed rows (`surface`, `effect`, `availability`)

Rows marked `present` must point to native dispatch, projection, docs, and verification evidence. Rows marked `partial`, `missing`, or `clashing` must describe the blocker/stage and must not appear as enabled placeholder commands. Registry-backed dollar workflow entries also record `CommandSpec::dollar_aliases`; the TUI `$` overlay and dispatch path derive those names from the registry rather than keeping a separate dollar-to-slash mapping table. Escaped `$$` renderings are normalized to `$` in inventory ids and aliases so dollar workflow skills and slash-agent prompts keep their canonical prefixes.

## Authority boundary

The inventory is evidence, not runtime state. Runtime command identity and dispatch live in `crates/harness-core/src/command_registry.rs`; replay remains projection-only; TUI and CLI surfaces derive from registry/matrix adapters rather than duplicating command lists.

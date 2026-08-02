# Skill activation offline recipe

## Purpose

Verify the real `task(load_skills=…)` and `skill` activation path for multiple skills without writing new tests. This recipe points agents at the existing deterministic nextest owners that cover resolve order, deduplication, missing skill rejection, and disabled/denied/malformed handling before a child agent spawns.

## Use When

Use after product-touching changes to skill catalog loading, skill frontmatter/resources, `task` delegation, or any path that could affect how skills resolve before child spawn.

## Do Not Use When

Do not use this recipe as a substitute for the owner nextest lanes. Do not use it to claim live skill-load smoke, simulation matrix ownership, or skill "quality" evaluation. Do not reimplement these assertions in a one-off script.

## Execution Policy

Stay offline. Run the existing owner tests and trust their coverage. If they fail, fix the source before claiming the skill activation path is healthy. Do not add redundant test logic to prove what the owners already prove.

## Steps

1. Run the task-tool owner tests that exercise `load_skills`:

   ```bash
   cargo nextest run -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test task_tool_
   ```

   Relevant owner assertions in `crates/harness-tools/tests/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/05_task_tool_rejects_missing_loaded_skill_test.rs`:

   - `task_tool_injects_shipped_builtin_skill_bodies_into_child_prompt` — multi-skill load with shipped built-ins (`git-master`, `review-work`, `frontend-ui-ux`) resolves in order and injects bodies into the child prompt.
   - `task_tool_rejects_disabled_shipped_builtin_skill_before_child_spawn` — a disabled stable id fails before the child is spawned.
   - `task_tool_preserves_requested_skill_order_and_deduplicates_loaded_context` — requested order is preserved and duplicate names deduplicate loaded context.
   - `task_tool_rejects_missing_loaded_skill_before_child_spawn` — a missing skill fails before the child is spawned.

2. Run the skill discovery and V1 contract owner tests:

   ```bash
   cargo nextest run -p harness-tools --test skill_load_discovery_test
   ```

   This covers catalog metadata, project/global discovery, resource redaction, and shipped built-in skill contracts.

3. Confirm both commands exit zero.

## Tool Usage

Use `bash` only to run the nextest commands. Use `read`/`grep` to inspect the test source or event excerpts only when diagnosing a failure.

## Escalation and Stop Conditions

Stop if either nextest command exits non-zero. Escalate to the `crates/harness-tools` owner tests if the failure is outside your change.

## Final Checklist

- `task_tool_` owner tests pass.
- `skill_load_discovery_test` passes.
- No new tests duplicate existing owner coverage.
- Non-claims stated: offline only, not live skill-load smoke, not simulation matrix ownership.

## Advanced Notes

Live skill-load smoke is optional and deferred. Document it as non-CI if it lands later. See `docs/harness-live-agent-testing-prd.md` workstream WS-L3 for the residual PRD disposition.

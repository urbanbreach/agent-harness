# Phase 2: Resolver And Config Polish

Use this file as a loose implementation prompt for an agent. This phase assumes Phase 1 has made the coordinator-owned child-agent task lane stable.

## Task

Improve the agent selection, category/profile resolution, skill-loading, and tool argument contract around orchestration while keeping the runtime model event-sourced and coordinator-owned.

## Expected Outcome

- `task` has a clear, documented contract for category-based delegation, direct subagent/profile selection, session continuation, skill injection, sync execution, and background execution.
- Config can express only the orchestration policy needed now.
- Parent-child profile policy is explicit and tested.
- The public config examples and generated schemas stay aligned.

## Required Context

Read these before editing:

- `AGENTS.md`
- `crates/harness-core/AGENTS.md`
- `crates/harness-tools/AGENTS.md`
- `docs/config.md`
- `configs/harness.example.jsonc`
- `configs/config.json`
- `crates/harness-core/src/config.rs`
- `crates/harness-core/src/config/public.rs`
- `crates/harness-core/src/agent.rs`
- `crates/harness-core/src/perm.rs`
- `crates/harness-tools/src/agent_ops.rs`
- `crates/harness-tools/src/control_plane.rs`

## Inspiration To Adapt

Adapt the useful parts of OMO's `delegate-task` design:

- Category selection resolves to a concrete profile/model.
- Direct subagent/profile selection remains possible.
- `load_skills[]` is explicit and visible in the prompt contract.
- `run_in_background=true` returns IDs immediately.
- `run_in_background=false` waits for the child result.
- `task_id` or `session_id` resumes an existing child session.

Do not import OMO's plugin-specific config shape wholesale.

## Must Do

- Define the smallest config contract needed for category/profile orchestration.
- Prefer existing config keys: `agent`, `default_agent`, `permission`, `skills`, model/profile metadata, and toolsets.
- Keep unsupported OpenCode keys explicitly rejected or compatibility-only as they are today.
- Preserve strict JSON schemas and `deny_unknown_fields` on tool args.
- Make parent-child profile policy clear: which profiles can redelegate, which profiles are read-only, and which child profiles are allowed.
- Ensure permission rules can target task agents through the existing task permission path.
- Make structured tool output stable enough for agents to consume.
- Document expected use of `load_skills` and `run_in_background` in the tool descriptions or agent prompt surface.
- Add tests for category fallback, unknown category/profile errors, skill loading failures, continuation by `task_id`, and model inheritance/override behavior.

## Must Not Do

- Do not add team specs, mailboxes, worktrees, or shutdown protocols in this phase.
- Do not add broad legacy compatibility shims for unreleased config shapes.
- Do not make `harness-tools` decide scheduling or permissions.
- Do not make tool schemas loose to match inspiration projects.
- Do not add dependencies.

## Likely Files

- `crates/harness-core/src/config.rs`
- `crates/harness-core/src/config/public.rs`
- `crates/harness-core/src/perm.rs`
- `crates/harness-core/src/agent.rs`
- `crates/harness-tools/src/agent_ops.rs`
- `crates/harness-tools/src/control_plane.rs`
- `crates/harness-tools/src/native_tools.rs`
- `configs/harness.example.jsonc`
- `configs/config.json`
- `docs/config.md`
- `.agent-harness/agents/*.md`

## Suggested Implementation Steps

1. Inventory current agent/profile/category parsing and defaults.
2. Decide whether missing resolver behavior belongs in core config resolution or in the task tool adapter.
3. Add tests that describe the desired resolver contract before changing behavior.
4. Implement the smallest resolver/config changes that satisfy those tests.
5. Update tool descriptions and prompt assets so agents know the exact required parameters.
6. Update config docs, examples, and generated schema artifacts if public config changed.

## Verification

Run targeted checks for config, tools, and docs:

```bash
cargo test -p harness-tools --test native_tool_parity_matrix
cargo test -p harness-tools
cargo test -p harness --test config_docs_reference
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo check --workspace
```

If schemas change, regenerate them through the established project command and verify the diff is intentional.

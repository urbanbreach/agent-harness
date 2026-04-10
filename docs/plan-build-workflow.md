# Shipped Plan -> Build workflow

The current shipped workflow is intentionally narrow. The harness now also treats `plan` and `build` as named first-class agents: if a config omits their `system_prompt`, bootstrap falls back to the shipped Plan/Build prompts instead of a generic profile intro.


1. start in `build`
2. switch to `plan` when you want a read-only planning pass
3. approve the plan explicitly
4. let `plan.exit` hand off back into `build`

This is the workflow issue #104 signs off. It is the default path that later orchestration work depends on.

For a fresh install/checkout, create the local bootstrap config first:

```bash
harness config init
```

That writes the shipped example to `./harness.jsonc`, after which bare `harness` uses the normal
auto-discovery path.

## Lane contract

### `build`
- default lane in `configs/harness.example.jsonc`
- implementation-focused
- may edit files, run shell verification, and delegate when it materially helps
- expected to finish with concrete evidence, changed files, what was not tested, and remaining risks

### `plan`
- explicit planning lane
- read-only in practice and in policy
- should inspect, ask questions, and produce a concrete implementation plan with scope, files, risks, and verification steps
- must not start implementation work
- may call `plan.exit` only after the user explicitly approves implementation

## How the handoff works

`plan.exit` is only exposed on plan-capable agents. When the user approves implementation, the harness:

1. confirms the switch to the configured target profile
2. emits a `plan_exit_handoff`
3. switches the next-turn profile to `build`
4. submits the approved implementation prompt into `build`

The canonical handoff prompt remains:

> The plan has been approved, you can now edit files. Execute the plan.

## CLI and TUI visibility

The shipped shell keeps the active lane visible through launch metadata and runtime identity:

- interactive config guidance says the harness defaults to `build` and keeps the `plan -> build` handoff available
- `/model` and the command palette can switch between configured profiles such as `build` and `plan`
- the live shell identity shows the active profile/provider/model tuple
- the `plan.exit` handoff test proves the next-turn profile flips from `plan` to `build`

## Canonical touchpoints

- `configs/harness.example.jsonc`
- `crates/harness/src/bootstrap.rs`
- `crates/harness-tools/src/control_plane.rs`
- `crates/harness-tui/src/app.rs`
- `crates/harness-tools/tests/native_control_plane_tools.rs`
- `crates/harness-tui/src/app/tests.rs`
- `crates/harness-testkit/tests/live_proxy_e2e.rs`

## What this signoff does not claim

This signoff covers the shipped Plan/Build path only. It does not claim that later roadmap items such as swarms, Ralph loops, `$` commands, or plugin-backed orchestration are done.

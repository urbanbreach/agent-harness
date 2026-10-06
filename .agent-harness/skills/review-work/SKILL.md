---
name: review-work
description: Review completed changes against the goal, repository constraints, and verification evidence.
argument_hint: review changed work against goal and evidence
allowed_tools: spawn_subagent, get_command_or_subagent_output, wait_commands_or_subagents, kill_command_or_subagent, bash, read, grep
mcp: none
resources:
---

# Review work

Use after substantial implementation or when the operator requests a review.
Supply the goal, changed files, expected behavior, and checks already run.

Choose the relevant bundled agents:

- `reviewer` checks correctness and maintainability. It can ask `scout` for local context.
- `security-reviewer` checks security-sensitive changes using repository evidence.
- `scout` researches local constraints or external documentation when needed.
- `task` performs hands-on QA. Explicitly assign the commands and scenarios to run;
  subagents otherwise leave verification to the parent.

Call `spawn_subagent` with `prompt`, `description`, the chosen `subagent_type`, and
`background: true`. Independent reviews can run together. Use returned IDs with
`get_command_or_subagent_output`; use `wait_commands_or_subagents` only when the
remaining work depends on those results. Do not guess IDs or poll repeatedly.

Verify findings against the code. Fix blocking defects, rerun affected checks,
and request another review only for unresolved concerns. Review output does not
replace executable QA evidence. Report remaining limitations plainly.

Stable id: `skill:project:review-work`.

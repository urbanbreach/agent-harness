---
name: team-mode
description: Usage guidance for declared teams, active team runs, mailbox/task coordination, and shutdown approval flow.
tools: [team_list, team_create, team_status, team_send_message, team_task_create, team_task_list, team_task_get, team_task_update, team_shutdown_request, team_shutdown_approve, team_shutdown_reject, team_delete]
commands:
  - team_list
permissions:
  task: ask
---

# Team mode

Use this skill when coordinating multiple Harness agents through declared teams or active team runs.

## Declared teams
- Declared teams live in `.agent-harness/teams/<name>.json` or the user Harness team directory.
- Run `team_list` first to inspect declared specs, validation warnings, active runs, and environment readiness.
- Validate lead/member eligibility before spawning; read-only members should stay in research roles.

## Active runs
- Keep team state event-sourced through the team tools.
- Use team tasks/mailbox messages for shared coordination instead of hidden side channels.
- Shutdown requires request plus approve/reject, and delete is only valid after a terminal team state.

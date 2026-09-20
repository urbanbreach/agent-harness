# Audit Index

| Audit | Source Commit | Status |
|---|---|---|
| [Performance audit](performance-audit/README.md) | `9abbd54f` | Six candidates and scoped settlement optimization implemented; [A/B results](performance-audit/RESULTS.md) available |

The performance follow-up preserves the audit's durability and canonical-projection
constraints. Its results include fresh baseline comparisons, scoped checks, manual
terminal QA, and eight full-suite failures reproduced on the pristine baseline.

## Subagent implementation plan

| Plan | Priority | Effort | Depends on | Status |
|---|---|---|---|---|
| [001: Give subagents independent role permissions](001-role-scoped-subagents.md) | P1 | M | None | Complete |
| [004: Evaluate file permissions against effective workspace targets](004-align-file-permissions-with-targets.md) | P1 | M | None | DONE |
| [005: Redact tool artifacts before writing them to disk](005-redact-persisted-tool-artifacts.md) | P1 | M | None | DONE |
| [006: Validate restore targets before changing workspace files](006-contain-workspace-restores.md) | P1 | M | 004 (DONE) | DONE |
| [007: Preserve existing file permissions during atomic edits](007-preserve-modes-on-atomic-edits.md) | P1 | S | None | DONE |
| [008: Preserve authored permission-rule order through configuration loading](008-preserve-authored-permission-order.md) | P1 | M | 002 (DONE) | DONE |
| [009: Keep malformed provider payloads out of errors and logs](009-remove-provider-payloads-from-errors.md) | P1 | S | None | DONE |
| [010: Reject malformed OAuth callback encoding without panicking](010-decode-oauth-callbacks-without-panics.md) | P1 | S | None | DONE |
| [011: Require a valid terminal outcome before completing provider streams](011-fail-incomplete-provider-streams.md) | P1 | M | 009 (DONE) | DONE |
| [017: Contain archive traversal and honor session inclusion](017-contain-archive-traversal.md) | P1 | M | None | DONE ([#240](https://github.com/urbanbreach/agent-harness/issues/240)) |

Planned at `062e5ea8` on 2026-09-14 after comparing the local OpenCode and Senpi
references. The user confirmed independent child-role permissions under shared
project restrictions. The plan keeps the existing profiles and task runtime;
custom role formats, team orchestration, and scheduler changes are out of scope.

Implementation and the approved fixture updates are complete. Scoped behavioral
checks, simulation and xterm.js proof pass. The final full suite has 4,613 passes
and three independently reproduced baseline failures; the existing plan/index
branding gate also remains failing. See the plan's delivery evidence.

A subsequent user-requested follow-up enables bash, LSP, skill loading, and
configured MCP discovery for both research roles; their native edit and delegation
denies and the shared-policy ceiling remain. The current contract is in
[generic agent and tasks](../docs/operations/generic-agent-and-tasks.md).

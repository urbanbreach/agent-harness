# Compliance Matrix (latest)

Generated: 2026-03-02

## Plan A: foundation (tasks 1-26)

Status: **PASS** for tasks 1-26 (optional task 27 out of scope).

| Task | Status | Evidence |
|---|---|---|
| 1 | PASS | `task-1-workspace-ci.txt`, `task-1-gitlab-ci.txt` |
| 2 | PASS | `task-2-config-validate.txt`, `task-2-config-invalid.txt`, `task-2-schema.txt` |
| 3 | PASS | `task-3-clock-tests.txt`, `task-3-clock-deterministic.txt` |
| 4 | PASS | `task-4-redact-tests.txt`, `task-4-secretscan.txt` |
| 5 | PASS | `task-5-event-snapshots.txt`, `task-5-event-redaction.txt` |
| 6 | PASS | `task-6-store-tests.txt`, `task-6-store-corruption.txt` |
| 7 | PASS | `task-7-coordinator-tests.txt`, `task-7-no-redelegate.txt` |
| 8 | PASS | `task-8-scheduler.txt`, `task-8-stale.txt` |
| 9 | PASS | `task-9-permissions.txt`, `task-9-headless-deny.txt` |
| 10 | PASS | `task-10-tools.txt`, `task-10-tools-gating.txt` |
| 11 | PASS | `task-11-hashline-tests.txt`, `task-11-hashline-crlf.txt` |
| 12 | PASS | `task-12-hashline-tool.txt`, `task-12-hashline-permission.txt` |
| 13 | PASS | `task-13-mock-provider.txt`, `task-13-mock-provider-error.txt` |
| 14 | PASS | `task-14-openai-compatible-wiremock.txt`, `task-14-no-leak.txt` |
| 15 | PASS | `task-15-agents.txt`, `task-15-agent-cancel.txt` |
| 16 | PASS | `task-16-golden-digest.txt`, `task-16-missing-permission.txt` |
| 17 | PASS | `task-17-replay.txt`, `task-17-meta.txt` |
| 18 | PASS | `task-18-projections.txt`, `task-18-replay-no-tools.txt` |
| 19 | PASS | `task-19-tui-snapshots.txt`, `task-19-tui-keymap.txt` |
| 20 | PASS | `task-20-tui-live.txt`, `task-20-tui-grouping.txt` |
| 21 | PASS | `task-21-tui-replay.txt`, `task-21-tui-replay-no-tools.txt` |
| 22 | PASS | `task-22-permission-ui.txt`, `task-22-permission-deny.txt` |
| 23 | PASS | `task-23-diff-ui.txt`, `task-23-diff-missing.txt` |
| 24 | PASS | `task-24-pty-e2e.txt`, `task-24-pty-repeat.txt` |
| 25 | PASS | `task-25-gitlab-ci.txt`, `task-25-local-parity.txt` |
| 26 | PASS | `task-26-docs.txt`, `task-26-example-config.txt` |

## Plan B: compliance remediation (tasks 1-12)

Status: **PASS** for tasks 1-12.

| Task | Status | Evidence |
|---|---|---|
| 1 | PASS | `task-1-fmt.txt`, `task-1-clippy.txt`, `task-1-test.txt`, `task-1-deterministic-tests.txt` |
| 2 | PASS | `task-2-coord-subscribe.txt` |
| 3 | PASS | `task-3-diff-refs-test.txt` |
| 4 | PASS | `task-4-tui-help.txt` |
| 5 | PASS | `task-5-tui-replay.txt` |
| 6 | PASS | `task-6-tui-live.txt` |
| 7 | PASS | `task-7-permission-ui.txt` |
| 8 | PASS | `task-8-diff-ui.txt` |
| 9 | PASS | `task-9-pty-e2e.txt`, `task-9-pty-repeat.txt` |
| 10 | PASS | `task-10-gitlab-ci.txt` |
| 11 | PASS | `task-11-headless-run.txt`, `task-11-replay.txt`, `task-11-pty-e2e.txt` |
| 12 | PASS | `task-12-evidence-links.txt` |

## Final verification wave

| Check | Status | Evidence |
|---|---|---|
| F1 Plan Compliance Audit | PASS | Oracle session `ses_34ffd92dfffeK3B5IWipOkOfu8` |
| F2 Code Quality Review | PASS with noted risks | Session `ses_34ff3d76affeXRMbnkdtPYG6A5` |
| F3 Real Manual QA (Agent-Driven) | PASS | Session `ses_34ff026f2ffeUEcXXpWf2mwxP4` |
| F4 Scope Fidelity Check | PASS | Session `ses_34feed6f8ffeJCzKkhCUxuJEqB` |

## Residual risks (from F2 review)

- Live TUI path uses unbounded in-memory event accumulation and an unbounded std mpsc channel.
- Some coordinator paths intentionally swallow handler errors (`let _ = ...`) and could reduce observability under write failures.
- `SubscriberLagged` recovery path lacks dedicated test coverage.

# Harness OMX Next Completion Dossier

This dossier is the public closeout map for the Harness-native OMX command-parity slice. It summarizes the acceptance anchors that must stay aligned with the runtime registry, workflow inventory, docs, and verification evidence.

## Acceptance map

- **Ultragoal ledger** — `.omx/ultragoal/ledger.jsonl` is the durable story audit trail; intermediate stories keep the aggregate Codex goal active until the final G008 final cleanup/review gate.
- **Workflow-first single-operator model** — `operator` is the visible default primary lane; legacy `build`, `plan`, and `discipline` profiles remain hidden compatibility/escalation profiles.
- **Projection-only CLI/TUI/replay** — replay, sessions, dossier, and status readers derive from stored events and projections rather than rerunning hooks or tools.
- **OMO cut/quarantine** — legacy OMO/parity material is preserved only as background migration evidence, not the product direction.
- **Permission/question/signoff correctness** — mutating workflows route through coordinator-owned events, permission names, question handling, and signoff/closeout policy checks.
- **Team/subagent as escalation** — team and slash-agent paths are bounded escalation surfaces; workers and slash agents cannot bypass coordinator scheduling or redelegate unless a profile explicitly allows it.
- **Config/runtime split** — public config and generated schemas document runtime knobs, while command behavior derives from `harness-core::command_registry` and evidence metadata from the workflow inventory.
- **Verification evidence** — every completed story records targeted command/test evidence; broader verification is collected in `docs/harness-omx-next-verification-dossier.md`.
- **Completion dossier** — this file links the acceptance criteria to final closeout and must be kept in sync with docs/config.md, docs/testing.md, README.md, and the inventory fixture.

## Final gate and blockers

- **G008 final cleanup/review gate** — completed for this slice with cleanup evidence at `target/ultragoal/ai-slop-cleaner-report.md`, review evidence at `target/ultragoal/code-review-report.md`, and structured gate evidence at `target/ultragoal/final-quality-gate.json`.
- **Applicable `$` command parity** — all applicable reference `$` commands must remain searchable, dispatchable, coordinator-owned, and tested through the command registry, TUI adapters, workflow evidence, continuations, or task-tool slash-agent shortcuts.
- **Non-applicable worker protocol** — `worker` is the only non-present reference skill because it is an internal team-pane protocol, not a user-facing Harness `$` command. Deprecated shim skills such as `ask-claude`, `ask-gemini`, and `build-fix` remain present as compatibility workflow commands.
- **No placeholder dispatch** — applicable command families must never regress to prompt-only, shell-only, `BlockedWorkflow`, or “not executable yet” placeholders.

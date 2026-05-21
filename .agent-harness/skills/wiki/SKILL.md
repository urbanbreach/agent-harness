---
name: wiki
description: Persistent markdown project wiki stored under repository harness_wiki with keyword search and lifecycle capture
triggers: ["wiki add", "wiki lint", "wiki query", "wiki read", "wiki delete"]
---

# Wiki

Persistent, self-maintained markdown knowledge base for project and session knowledge.

## Operations

### Ingest
```bash
harness workflow wiki wiki_ingest --input '{"title":"Auth Architecture","content":"...","tags":["auth","architecture"],"category":"architecture"}' --json
```

### Query
```bash
harness workflow wiki wiki_query --input '{"query":"authentication","tags":["auth"],"category":"architecture"}' --json
```

### Lint
```bash
harness workflow wiki wiki_lint --json
```

### Quick Add
```bash
harness workflow wiki wiki_add --input '{"title":"Page Title","content":"...","tags":["tag1"],"category":"decision"}' --json
```

### List / Read / Delete
```bash
harness workflow wiki wiki_list --json
harness workflow wiki wiki_read --input '{"page":"auth-architecture"}' --json
harness workflow wiki wiki_delete --input '{"page":"outdated-page"}' --json
harness workflow wiki wiki_refresh --json
```

## Categories
`architecture`, `decision`, `pattern`, `debugging`, `environment`, `session-log`, `reference`, `convention`

## Storage
- Pages: `harness_wiki/*.md`
- Index: `harness_wiki/index.md`
- Log: `harness_wiki/log.md`

## Cross-References
Use `[[page-name]]` wiki-link syntax to create cross-references between pages.

## Auto-Capture
At session end, discoveries can be captured as `session-log-*` pages. Configure via `wiki.autoCapture` in `harness.jsonc`.

## Hard Constraints
- No vector embeddings — query uses keyword + tag matching only
- Wiki files are repository project knowledge under `harness_wiki/`; legacy `harness_wiki/` is read-only compatibility input when no canonical wiki exists

## Harness substrate override

When this skill is loaded by `agent-harness`, the workflow protocol above is the behavioral source, but the runtime substrate differs from Harness:

- Use coordinator-owned workflow events, workflow projections, task records, and evidence artifacts as the authority.
- Do **not** write or mutate per-mode `Harness workflow projection/*.json` files; lifecycle, phase, continuation, and closeout state are event-sourced by the harness.
- Translate Harness CLI/state operations to harness-native surfaces when needed: workflow evidence/status/goal/wiki CLI commands, native `task`/team tools, and workflow projections.
- Treat native terminal UI-specific Harness team/question instructions as conceptual guidance unless the harness exposes an equivalent native tool; prefer the harness native tool surface.
- Keep final claims evidence-backed: changed files, commands run, artifacts/evidence refs, remaining risks, and the stop condition reached.

## Harness state contract

Harness workflow state is authoritative through coordinator-owned events, workflow projections, native tool artifacts, and recorded workflow evidence. Skills must not require external state files, terminal-pane routing, or upstream CLI lifecycle commands as proof of progress.

## Execution protocol

Use the native Harness command dispatch, question, team, task, evidence, and verification surfaces named by the active workflow. Treat compatibility references as historical context only, and translate them into coordinator-owned actions before acting.

## Evidence and closeout contract

Record material progress as workflow evidence with artifact paths or command output summaries. Close only after the relevant checks pass, pending tasks are resolved or explicitly aborted, and the operator-facing status can be replayed from Harness events.

## Stop/escalation conditions

Stop when the workflow objective is verified complete, cancelled by the operator, or blocked by missing authority. Escalate only for destructive, credentialed, external-production, or materially scope-changing choices.

## Verification checklist

- Native Harness workflow projection reflects the expected mode/status.
- Required evidence artifacts or command summaries are recorded.
- Targeted tests, lint, docs checks, or visual/review gates named by the workflow have fresh results.
- No external state-file, terminal multiplexer, or upstream CLI command is the proof boundary.

## Purpose

Provide a native Harness workflow protocol for this skill so command dispatch, state projection, evidence, and closeout remain coordinator-owned and replayable.

## Use when

Use this skill when the matching `$` workflow command or catalog entry is selected and the operator request fits the workflow description.

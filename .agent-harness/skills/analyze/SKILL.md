---
name: analyze
description: "Run read-only deep repository analysis and return a ranked synthesis with explicit confidence, concrete file references, and clear evidence-vs-inference boundaries. Use when a user says 'analyze', 'investigate', 'why does', 'what's causing', or needs grounded cross-file explanation before any changes are proposed."
---

# Analyze — Read-Only Deep Analysis

Use this skill to answer the user’s question through **read-only repository analysis**. The goal is to explain what the codebase most likely says about the question, not to drift into implementation, debugging theater, or generic fix planning.

## Use `$analyze` when

- the user wants a grounded explanation, not code changes
- the answer requires reading multiple files or tracing behavior across boundaries
- there are several plausible explanations and they need to be ranked
- confidence should reflect the strength of the available evidence
- the user wants to understand architecture, behavior, causality, impact, or tradeoffs before changing anything

Examples:
- why a workflow behaves a certain way
- how a feature is wired across modules
- what likely explains a failure, regression, or mismatch
- what would be impacted by changing a dependency or contract
- which interpretation of the current codebase is best supported

## Do not use `$analyze` when

- the user explicitly wants code edits, a fix, or execution — use the appropriate implementation lane instead
- the user wants a new product plan or acceptance criteria — use `$plan` / `$ralplan`
- the request is a simple one-file fact lookup — read the file and answer directly
- the request is purely about running the Harness native terminal UI team runtime — use `$team` only when Harness runtime is active

## Non-negotiable contract

Analyze is **read-only by contract**.

- Do not edit files.
- Do not turn the answer into an implementation plan.
- Do not recommend fixes as the primary output.
- Do not silently switch into execution work.
- Do not overclaim certainty.
- Do not invent facts that are not supported by repository evidence.
- Do not use judgmental, normative, or speculative language that outruns the evidence.

If a next step is helpful, keep it to a **discriminating read-only probe** that would reduce uncertainty.

## Question-aligned synthesis

Answer the user’s actual question first.

- Start from the asked question, not a generic debugger template.
- Keep the synthesis scoped to what the user needs to know.
- Scale the depth to the request: for simple or obvious questions, reduce swarm intensity and answer directly after enough reading.
- For broader questions, expand the search surface but keep the final answer tightly synthesized.

## Evidence rules

Maintain an explicit **evidence-vs-inference distinction**. Every material claim must be labeled as one of:

1. **Evidence** — directly supported by concrete repository artifacts
2. **Inference** — a reasoned conclusion drawn from evidence
3. **Unknown** — a question the current repository evidence does not resolve

Never present an inference as if it were direct evidence.
Never present a guess as if it were an inference.
Call out uncertainty explicitly when the codebase does not settle the question.

### Acceptable evidence

Prefer stronger evidence over weaker evidence:

1. direct code paths, contracts, tests, generated artifacts, configs, or docs with concrete file references
2. multiple independent files pointing to the same conclusion
3. localized behavioral inference from well-supported code structure
4. weaker contextual clues that remain explicitly marked as tentative

Unsupported speculation is not evidence.

## Parallel exploration policy

Parallel exploration is allowed when it improves quality, but it must stay runtime-safe.

- Default to direct read-only analysis when the answer is simple.
- When parallelism helps, prefer **native subagents by default** or equivalent in-session parallel exploration when available.
- Keep parallel lanes bounded: each lane should answer a concrete sub-question or inspect a specific subsystem.
- Use **`$team` only when Harness runtime is active** and durable coordinator-native coordination is actually needed.
- Do not imply that `$team` is available in plain Codex/App sessions.

A good default split for complex analysis is:
- one lane for primary code path / contracts
- one lane for config / orchestration / generated surfaces
- one lane for tests / docs / secondary corroboration

## Execution policy

- Default to outcome-first progress and completion reporting: state the question, evidence, inference boundaries, and stop condition before adding process detail.
- Treat newer user task updates as local overrides for the active workflow branch while preserving earlier non-conflicting constraints.
- If the user says `continue`, keep working from the current analysis state instead of restarting discovery.

## Working method

1. Restate the question in one sentence.
2. Identify the smallest set of files most likely to answer it.
3. Read for direct evidence first.
4. If needed, open bounded parallel exploration lanes.
5. Compare competing explanations.
6. Rank the explanations by support.
7. Return a synthesis that clearly separates evidence from inference.

## Output contract

Structure the answer so the user can see what is known, what is inferred, and how confident the synthesis is.

### Question
[Restate the user’s question briefly]

### Ranked synthesis
| Rank | Explanation | Confidence | Basis |
|------|-------------|------------|-------|
| 1 | ... | High / Medium / Low | strongest supporting evidence |
| 2 | ... | High / Medium / Low | why it trails |
| 3 | ... | High / Medium / Low | why it remains possible |

### Evidence
- `path/to/file:line-line` — what this artifact directly shows
- `path/to/file:line-line` — corroborating evidence

### Inference
- What the evidence most strongly implies
- Why weaker alternatives were down-ranked

### Unknowns / limits
- What the repository evidence does not establish
- What would need to be checked next to reduce uncertainty

## Quality bar

A good analyze response is:
- read-only and question-aligned
- ranked rather than flat
- explicit about confidence
- concrete about file references
- careful about evidence vs inference
- free of unsupported speculation
- free of normative drift or judgmental filler
- explicit about the evidence-vs-inference distinction
- concise for simple cases, broader only when the question truly needs it

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

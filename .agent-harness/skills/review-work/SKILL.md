---
name: review-work
description: High-rigor post-implementation review workflow remapped to shipped Harness categories.
argument_hint: review changed work against goal and evidence
allowed_tools: task, background_output, background_cancel, bash, read, grep
target_agent: build
target_category: deep
mcp: none
resources: deferred-reference-not-loaded
---

# Review Work

## Purpose

Run a structured post-implementation review that checks goal fit, code quality, security, hands-on QA, and missing context before closeout.

## Use When

Use after significant implementation, refactors, security-sensitive changes, or when the operator asks to verify or review work.

## Do Not Use When

Do not use before there is changed work to review, for trivial one-line edits, or to replace the required test/manual QA evidence.

## Execution Policy

Use real Harness routes only. Launch independent reviewers with `task(category="ultrabrain", run_in_background=true, load_skills=[])`, `task(category="deep", run_in_background=true, load_skills=[])`, and `task(category="unspecified-high", run_in_background=true, load_skills=[])`. Collect each result with `background_output` and treat any conditional approval as rejection until fixed.

## Steps

1. Summarize the requested goal, changed files, scenarios, and verification already run.
2. Ask `category="ultrabrain"` to verify constraints, invariants, and hidden failure modes.
3. Ask `category="deep"` to review implementation quality and maintainability.
4. Ask `category="unspecified-high"` to run hands-on QA or context mining from local source and git state.
5. Fix blocking findings, rerun targeted verification, and repeat review only for affected concerns.

## Tool Usage

Use `task(..., run_in_background=true, load_skills=[])` for parallel review roles and `background_output` after completion. Use `bash` only for git/test status and never to edit files.

## Escalation and Stop Conditions

Stop if a reviewer reports an unresolved correctness, safety, permission, data-loss, or evidence gap. Do not declare completion on "looks good but" feedback.

## Final Checklist

- Goal and scenarios matched.
- All blocking findings fixed.
- Targeted tests and manual surface evidence rerun.
- Review outputs are summarized without bloating parent context.

## Advanced Notes

Stable id: `skill:project:review-work`. The review routes are category subagents from the shipped AgentCatalog; no non-shipped agents are referenced.

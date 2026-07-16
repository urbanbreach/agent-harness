---
name: harness-qa
description: Offline agent dogfood for product-touching harness changes using the mock golden_path channel and gitignored QA evidence.
argument_hint: offline dogfood | self-test evidence
allowed_tools: bash, read, grep
target_agent: build
target_category: deep
mcp: none
resources: references/evidence-convention.md
---

# Harness QA

## Purpose

Run offline agent dogfood against the real harness binary and leave reviewable evidence under `artifacts/qa-evidence/<YYYYMMDD>-<slug>/`. Prefer `scripts/harness-qa-dogfood.sh` so isolation, event capture, and secret fail-closed scanning stay consistent.

## Use When

Use after product-touching runtime, CLI, tool, scenario, or session-path changes that should prove the offline mock multi-step path still wires tools and writes inspectable events.

## Do Not Use When

Do not use for live provider proof, PTY/native visual signoff, simulation-matrix ownership, Docker isolation, or as a substitute for owner nextest lanes already covering the change.

## Execution Policy

Stay offline and deterministic. Isolate session roots under the evidence directory or `/tmp` (never `$HOME/.config/harness`). Run the dogfood script from the repo root; do not invent a parallel mock path. Treat secret material in evidence as a hard failure.

## Steps

1. Confirm the change is product-touching and offline dogfood is the right claim class.
2. From the repo root, run `bash scripts/harness-qa-dogfood.sh --self-test` (or the script with an explicit evidence slug when documenting a named run).
3. Verify evidence contains `README.md`, `commands.log`, `isolation-receipt.txt`, `events-excerpt.jsonl`, `lane-or-run-summary.txt`, and that isolation receipt paths stay under evidence or `/tmp`.
4. Summarize WHAT/OBSERVED/WHY/OMITTED plus explicit non-claims (not live; not PTY/native; not simulation matrix ownership).
5. Point the progress ledger or review notes at the evidence directory path without committing secrets or evidence trees.

## Tool Usage

Use `bash` only to invoke `scripts/harness-qa-dogfood.sh` and related read-only checks. Use `read`/`grep` to inspect evidence files and event excerpts. Do not use shell for source edits or secret-bearing logs.

## Escalation and Stop Conditions

Stop if the dogfood command fails, isolation receipt shows writes outside evidence/`/tmp`, secret patterns appear (`sk-`, `Bearer `, `BEGIN PRIVATE KEY`), or the operator asks for live/PTY/native claims this channel cannot prove.

## Final Checklist

- Offline dogfood command exited 0.
- Evidence directory exists under gitignored `artifacts/qa-evidence/`.
- Isolation receipt proves session-dir isolation.
- Secret scan clean.
- Non-claims stated (not live; not PTY/native; not simulation matrix ownership).

## Advanced Notes

Stable id: `skill:project:harness-qa`. This is a disableable built-in offline dogfood channel, not live signoff and not ownership of `docs/simulation-matrix.json` or the simulation lane.

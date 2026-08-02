---
name: harness-qa
description: Offline agent dogfood plus opt-in live smoke for product-touching harness changes with gitignored QA evidence.
argument_hint: offline dogfood | live smoke | self-test evidence
allowed_tools: bash, read, grep
target_agent: build
target_category: deep
mcp: none
resources: references/evidence-convention.md, references/skill-activation-recipe.md
---

# Harness QA

## Purpose

Run offline agent dogfood against the real harness binary and leave reviewable evidence under gitignored `artifacts/qa-evidence/`. Prefer the shipped scripts so isolation, event capture, budgets (live), and secret fail-closed scanning stay consistent.

- **Offline channel (default / mandatory for product-touching changes):** `scripts/harness-qa-dogfood.sh` → `artifacts/qa-evidence/<YYYYMMDD>-<slug>/`
- **Live-smoke channel (opt-in only):** `scripts/harness-qa-live-smoke.sh` → `artifacts/qa-evidence/<YYYYMMDD>-live-<slug>/`

## Use When

**Offline:** after product-touching runtime, CLI, tool, scenario, or session-path changes that should prove the offline mock multi-step path still wires tools and writes inspectable events.

**Live (opt-in):** when `HARNESS_LIVE_PROXY=1` and the live config/provider/model tuple are present, and you need budgeted fixed-smoke transport/auth proof with redacted evidence after a product-touching change that could affect live transport. Live is never a substitute for offline dogfood or owner nextest.

## Do Not Use When

Do not use offline for live provider proof. Do not use live without the required env (fail-closed exit is expected, not success). Do not use either channel for PTY/native visual signoff, simulation-matrix ownership, Docker isolation, freestyle open-ended eval missions as proof, or as a substitute for owner nextest. Do not claim native tool behavioral matrix ownership from live smoke (T5).

## Execution Policy

Isolate session roots under the evidence directory or `/tmp` (never `$HOME/.config/harness`). Run scripts from the repo root. Treat secret material in evidence as a hard failure. Offline stays deterministic/mock. Live requires explicit env and stays opt-in (not CI default). Missing live env must exit non-zero; never soft-skip into success.

## Steps

### Offline channel

1. Confirm the change is product-touching and offline dogfood is the right claim class.
2. From the repo root, run `bash scripts/harness-qa-dogfood.sh --self-test` (or `--slug <name>`).
3. Verify evidence contains `README.md`, `commands.log`, `isolation-receipt.txt`, `events-excerpt.jsonl`, `lane-or-run-summary.txt`, and isolation paths under evidence or `/tmp`.
4. Summarize WHAT/OBSERVED/WHY/OMITTED plus non-claims (not live; not PTY/native; not simulation matrix ownership).
5. Point the progress ledger at the evidence path without committing secrets or evidence trees.

### Live-smoke channel (opt-in)

1. Confirm live env is intentional: `HARNESS_LIVE_PROXY=1`, readable `HARNESS_LIVE_PROXY_CONFIG`, plus provider and model.
2. Without env, prove fail-closed: `bash scripts/harness-qa-live-smoke.sh --self-test-fail-closed` (must exit non-zero).
3. With env: `bash scripts/harness-qa-live-smoke.sh --slug <short-slug>` (default slug `live-smoke`).
4. Verify evidence under `artifacts/qa-evidence/<YYYYMMDD>-live-<slug>/` includes README, commands.log, isolation-receipt, budget-receipt, events-excerpt, secret-scan, lane-or-run-summary.
5. State non-claims: not tool matrix ownership (T5); not freestyle quality; not multi-provider matrix; not PTY/native; not a substitute for offline dogfood.

## Tool Usage

Use `bash` only to invoke `scripts/harness-qa-dogfood.sh`, `scripts/harness-qa-live-smoke.sh`, and related read-only checks. Use `read`/`grep` to inspect evidence files and event excerpts. Do not use shell for source edits or secret-bearing logs.

## Escalation and Stop Conditions

Stop if a dogfood/live-smoke command fails unexpectedly, isolation receipt shows writes outside evidence/`/tmp`, secret patterns appear (`sk-`, `Bearer `, `BEGIN PRIVATE KEY`), live is requested without env (fail-closed is correct), or the operator asks for tool-matrix / freestyle / PTY-native claims these channels cannot prove.

## Final Checklist

- Offline dogfood command exited 0 when claiming offline channel success.
- Live claims only when env present + live script green + evidence path + secret scan clean; otherwise fail-closed or blocked_external.
- Evidence under gitignored `artifacts/qa-evidence/` (live uses `*-live-*` slug prefix).
- Isolation receipt proves session-dir isolation.
- Secret scan clean.
- Non-claims stated for the channel used.

## Advanced Notes

Stable id: `skill:project:harness-qa`. Offline is the mandatory product-touching channel; live-smoke is opt-in only. Neither owns `docs/testing/simulation-matrix.json` or the simulation lane. For multi-skill activation verification, see `references/skill-activation-recipe.md`. Evidence layout: `references/evidence-convention.md`.

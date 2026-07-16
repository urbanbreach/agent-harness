# QA evidence convention

## Offline dogfood

Offline dogfood evidence lives under the gitignored root:

```text
artifacts/qa-evidence/<YYYYMMDD>-<slug>/
  README.md                 # WHAT / OBSERVED / WHY / OMITTED / non-claims
  commands.log              # exact commands + exit codes
  isolation-receipt.txt     # session-dir under evidence or /tmp; not $HOME/.config/harness
  events.jsonl              # full deterministic run events (local only)
  events-excerpt.jsonl      # first N lines for review
  lane-or-run-summary.txt   # short pass/fail + run dir
```

Produce evidence with:

```bash
bash scripts/harness-qa-dogfood.sh --self-test
```

Non-claims: not live provider proof; not PTY/native visual signoff; not simulation matrix ownership. Do not commit secrets or evidence trees.

## Live smoke / live agent dogfood

Live evidence uses the same root with a **`live-` slug prefix**:

```text
artifacts/qa-evidence/<YYYYMMDD>-live-<slug>/
  README.md                 # WHAT / OBSERVED / WHY / OMITTED + T5 non-claims
  commands.log
  isolation-receipt.txt
  budget-receipt.txt        # turns/time; cost if available else unmetered
  events-excerpt.jsonl      # redacted/capped
  secret-scan.txt
  lane-or-run-summary.txt
  fail-closed-receipt.txt   # only when fail-closed without env (optional)
```

Produce evidence with:

```bash
# Fail-closed without live env (must exit non-zero):
bash scripts/harness-qa-live-smoke.sh --self-test-fail-closed

# With live env:
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=harness.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=<provider> \
HARNESS_LIVE_PROXY_MODEL=<model> \
bash scripts/harness-qa-live-smoke.sh --slug <short-slug>
```

Lane stage artifacts for `signoff-live` may also land under `target/test-lanes/<run-id>/…`.

Non-claims for live: not native tool behavioral matrix ownership (T5); not freestyle quality; not multi-provider matrix; not PTY/native; not a substitute for offline dogfood; not CI default. Do not commit secrets or evidence trees.

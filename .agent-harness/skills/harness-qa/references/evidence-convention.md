# QA evidence convention

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

Non-claims for this channel: not live provider proof; not PTY/native visual signoff; not simulation matrix ownership. Do not commit secrets or evidence trees.

# Report consistency validation receipt

Command:

```bash
python3 .omo/ulw-research/20260831-044447/validate_report.py \
  .omo/ulw-research/20260831-044447/grok-build-harness-parity-audit.md \
  .omo/ulw-research/20260831-044447/claim-graph.md \
  .omo/ulw-research/20260831-044447/debate-log.md
```

Final exit status: `0`

Final output:

```text
PASS: report contract and evidence ledgers are consistent
```

The same validator failed against the initial draft skeleton before research
synthesis, establishing the requested RED baseline.

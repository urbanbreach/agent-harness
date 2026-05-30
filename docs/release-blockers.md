# V1 release blockers

This taxonomy separates release blockers from local development aids. Blockers must have deterministic or env-gated lane evidence before release claims are made.

| Category | Release blocker? | Lane or command |
|---|---:|---|
| correctness | yes | `fast`, `integration`, `simulation` |
| safety | yes | `quality-gates`, permission/tool tests |
| UX | yes for V1 operator flows | `signoff-pty`, `harness-tui` tests |
| docs | yes for public contracts | `fast` docs-reference tests, `quality-gates` static docs/test-suite guards |
| provider | yes for supported path claims | provider faux tests in `fast`, `signoff-live` only for live claims |
| performance | yes for release-facing budgets | `perf` |
| evidence | yes | `all-deterministic`, `signoff-binary`, `perf`, lane artifact roots, and `docs/claim-evidence-matrix.md` |

## Local development aids

Formatter output, local debug commands, exploratory scripts, and live stress lanes are aids unless the release claim cites them. A green doctor report is runtime health, not full roadmap completion.

## Lane mapping

`scripts/test-lanes.sh` is the source for lane names: `fast`, `integration`, `quality-gates`, `simulation`, `perf`, `signoff-pty`, `signoff-binary`, `signoff-live`, `signoff-native`, `stress-offline`, and `stress-live`.

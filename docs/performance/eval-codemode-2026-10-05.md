# Eval code-mode follow-up — 2026-10-05

The live measurements do not show a consistent speed or token benefit from the
new eval guidance. All 24 workflows returned correct answers; the edit cases
also passed an independent quantity/default/empty-input check. Tool routing stays
opt-in.

## Live workflow measurements

`scripts/qa/eval-workflows.py` ran the actual `openai-codex/gpt-6-sol` provider at
low reasoning effort, with a ten-request limit and 240-second process deadline.
Each suite used ABBA order for three synthetic tasks: inspect six service
manifests, filter/aggregate six order shards, and fix then verify a Python price
calculation. Each arm has two samples per task. The second suite added stronger
instructions to filter results inside the cell. Both suites are retained.

The baseline is `5a6b3ff74f17e39c5bd426a73b3ec6456f8487f0`. The frozen candidate
contains the prompt, tool-description, and optional routing work before the later
kernel/package/pool features. Both arms exposed the same tools; neither forced
routing. These measurements evaluate tool choice and guidance, not exclusive
routing or the final feature set. The final hashline-parsing clarification and
new helper descriptions were added after measurement.

| Suite | Task | Baseline: seconds / tokens / requests | Candidate: seconds / tokens / requests |
| --- | --- | --- | --- |
| live | research | 11.34 / 7,267 / 4 | 14.37 / 11,778 / 4 |
| live | filter | 16.13 / 120,640 / 4 | 27.79 / 187,142 / 4 |
| live | edit | 23.42 / 14,470 / 6.5 | 20.15 / 13,661 / 6 |
| live-filtered | research | 16.97 / 6,395 / 3.5 | 13.06 / 9,612 / 4 |
| live-filtered | filter | 17.15 / 65,729 / 4 | 22.22 / 110,418 / 4 |
| live-filtered | edit | 23.26 / 12,798 / 6 | 25.58 / 17,032 / 6 |

Values are arithmetic means. Tokens are the provider's total reported tokens
summed across requests, not estimated cost. Usage was present for every request.
Cache-read counts are retained even though the configuration requested no cache
retention; this was not a guaranteed cold-cache experiment. Timing includes
provider/network variability and tool/runtime startup. Two samples per arm do
not establish a statistically reliable performance difference.

Inspection of these synthetic runs showed that cells sometimes displayed entire
read results and only filtered them in a later cell. Read results also include
hashline prefixes, which need handling before JSON parsing. Grouping calls alone
does not reduce the text subsequently sent to the model. The second guidance
variant still did not consistently reduce usage, so no performance claim or
default visibility change follows from these results.

The [aggregate JSON](eval-codemode-2026-10-05.json) records every sample, binary
and prompt SHA-256, reported usage, cache counts, correctness, and top-level tool
counts. Nested coordinator calls are not included in those top-level counts.
Provider payloads, credentials, and session journals are not included.

Reproduce with separately built baseline/candidate binaries and their matching
prompt files:

```sh
python3 scripts/qa/eval-workflows.py \
  --baseline /path/to/baseline-harness --candidate /path/to/candidate-harness \
  --baseline-prompt /path/to/baseline-gpt-6.md \
  --candidate-prompt /path/to/candidate-gpt-6.md \
  --output /tmp/eval-workflows
```

## Capability verification

The combined source passed these fresh checks:

| Check | Result |
| --- | --- |
| Workspace nextest, all features | 1,976 passed; 22 opt-in tests skipped (`b21abb6a-0717-4640-bc88-b2a7a5e8a3bd`) |
| Four-language eval lane on Node 24.0.0 | 9 engine tests and 11 coordinator/MCP tests passed (`ad48c040-aa42-4a6a-b298-1d08bbadedc9`, `734d3d7f-9899-44f4-89c5-b2d674da52c9`) |
| Release engine/conformance run | Passed; all 54 cells match the prior Node baseline, excluding expected exception wording (`c6519edc-655e-408a-b763-aa86f0b9eed2`) |
| PTY/xterm | 12 captures passed at 40, 80, and 120 columns; no surviving process group or temporary sockets |
| Formatting, workspace check, Clippy all targets/features, CLI build | Passed; generated configuration schema matches |
| Static test-suite gates and diff whitespace | Passed |

The eval lane used Node 24.0.0, Python 3.13.16, Ruby, and Julia 1.13.1.
The Node 24 run includes actual managed npm/pip installs, so the minimum-version
check covers the new import/package paths too. The release conformance run used
Node 26.7.0. It produced engine timing samples as part of the existing runner;
those samples are not used to claim a speedup.

Visual inspection of the 40-column completed cell and 120-column expanded cell
confirmed code clipping, output disclosure, input/output gutters, and Unicode
alignment. Captures exercise ordinary eval lifecycle states with a scripted
provider; they are separate from the live-provider measurements above.

The branding gate still reports five pre-existing findings in
`docs/evidence/tui-rewrite/runtime-state/post-measure-host.json:37` and
`plans/001-role-scoped-subagents.md:30,64,65,160`. Those files are unchanged; the five findings were reproduced on the pristine
baseline worktree. No new branding findings remain.

The public-boundary tests cover:

- Provider definition routing, catalog refresh, nested read denial, and direct
  fallback when eval is absent or denied.
- JavaScript/Python kernel tools, inferred schema, parent/child reentrancy,
  definition removal/revision fencing, cancellation, and child tool scope.
- Script-relative imports and persistent globals; actual local npm and wheel
  installs; disabled npm lifecycle scripts; failed/cancelled install rollback;
  reset imports; unchanged project files.
- QuickJS isolation, fresh globals, permission checks, heap/CPU limits, host-wait
  cancellation, and preservation of the regular Node kernel.
- Native pool width, queued/running cancellation, failed admission, owner
  checks, inherited kernel tools, one redacted aggregate notification, and
  handle waiting/control behavior, including actor-side rejection of old handles
  after native child restart.

## Reference scope

The primary design reference was Senpi-codemode at
`4550f6ee9d8adf02fd7dac183adfced430fb6ed7`; the secondary OMP reference was
`d9ee5e6a31d621f50ac6610cd1cdadccfbf41b0c`. The implementation uses Harness's
existing native scheduling, permissions, journals, and interpreter adapters.
It does not claim wire/API parity with either reference. Supported contracts
and limits are in [the eval guide](../tools/eval.md).

Fresh verification commands (the eval lane needs all four interpreters on PATH):

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --profile ci --workspace --all-features
HARNESS_EVAL_LANGUAGES=js,py,rb,jl scripts/test-lanes.sh eval
scripts/test-lanes.sh quality-gates
node scripts/qa/capture-eval.mjs /tmp/harness-xterm-codemode-visual
HARNESS_EVAL_PERF_OUTPUT=/tmp/harness-codemode-conformance.json \
  cargo nextest run --profile perf --release -p harness-eval \
  --test performance --run-ignored all
```

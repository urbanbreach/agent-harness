# Eval integration and code-mode capabilities

Requested on 2026-10-05. Baseline: `5a6b3ff7`.

Implement the full requested scope, using Senpi-codemode's public contracts as
the main reference and OMP where useful. Coordinator ownership,
permissions, cancellation, redaction, and replay guarantees continue to apply.

## Work and evidence

| Requirement | Status | Completion evidence |
| --- | --- | --- |
| Replace obsolete batch instructions and teach eval use when available | Complete | Prompt/schema checks; selective output and hashline parsing guidance |
| Configure model-facing tool visibility independently of authorization | Complete | Provider requests hide routed tools; eval invokes them under the same permissions; profiles without eval retain direct tools |
| Compare real model workflows before/after eval guidance | Complete | Repeated research, search/filter, and edit/verify tasks with model, correctness, usage, turns, and elapsed time recorded |
| Python-defined tools usable by child agents | Complete | Public coordinator test covers schema, execution during parent wait, scope, reset/redefinition, and cancellation |
| Load scripts and install Python/JS packages in managed environments | Complete | State persists; project files stay unchanged in managed mode; failed/cancelled installs preserve the previous environment |
| Optional isolated JavaScript cells | Complete | Fresh QuickJS context has no ambient filesystem/network/process access; host tools retain permissions; memory/time/cancellation limits work |
| Built-in worker pools and handle/wait controls | Complete | Bounded scheduling through coordinator-owned subagents, owned results, cancellation, and cleanup |
| Final integration and operator documentation | Complete | Schema/docs updated; 1,976 workspace tests; 20 eval checks on four interpreters and Node 24; 54 conformance cells; 12 terminal captures; format/check/Clippy pass. Static gates pass; five unchanged branding findings remain. |

Live results: 24/24 correct, with no consistent latency/token savings. Both
suites and their scope are retained in [the follow-up record](../docs/performance/eval-codemode-2026-10-05.md).

## Implementation order

1. Finish prompts and tool visibility; verify the provider/coordinator boundary.
2. Record model workflow measurements with existing runtime evidence kept distinct.
3. Add Python kernel tools and managed script/package commands.
4. Add isolated JavaScript and coordinator-owned worker pools/handles.
5. Verify the combined behavior and audit every requirement above against current
   source and fresh results before claiming completion.

Tests belong at existing public boundaries. Extend current cases where they
already own the behavior. Do not add a new testing framework or run historical
tools during replay. Live-provider measurements must name the actual provider
and must not be represented by scripted-provider results.

## References

- Senpi: `4550f6ee9d8adf02fd7dac183adfced430fb6ed7`,
  `packages/senpi-codemode/README.md` and owning source files.
- OMP: `d9ee5e6a31d621f50ac6610cd1cdadccfbf41b0c`,
  `docs/tools/eval.md` and owning source files.
- Existing Harness verification: `docs/performance/eval-node-2026-10-05.md`.
  These are baseline records, not verification of this work.

## Delivered contract

All requested capability groups are implemented. Pools use fresh native children
with bounded admission and one aggregate notification. Installs use managed npm
and pip revisions; `%load` preserves kernel globals. Rich controls target native
agent handles in JavaScript/Python; all four languages have wait/output and pool
adapters. Isolated JavaScript uses an opt-in QuickJS context.

This is not a drop-in upstream API replacement: keep-alive pools, project-mutating
install modes, Bun-specific commands, and completion handles are not exposed.
Limits and lifecycle details are documented in [the eval guide](../docs/tools/eval.md).

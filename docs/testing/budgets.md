# Performance budgets

These budgets cover the listed local fixtures. They do not establish performance
for larger production workloads. Run the performance lane to collect current measurements.

| Budget | Current local threshold | Evidence |
|---|---:|---|
| backend startup, streaming and idle CPU | measured local workload; no universal timing threshold | `scripts/measure-backend.py` |
| TUI render | warm 10,000-entry resize p95 below 8.333 ms; frame/resource comparisons measured separately | `cargo nextest run -p harness-tui` and the release perf profile |
| completed child sessions | final descriptor count at most four above startup | `scripts/measure-delegation.py` |
| binary size | no enforced threshold | no measurement gate yet |

## Backend workloads

The local streaming probe starts a fresh CLI process with an isolated workspace
and loopback provider. It measures launch to request, first output, stream and
journal completion, process CPU, idle CPU and peak resident memory. Each sample
must produce the exact expected output and a completed journal without provider
fragments. There is no startup threshold derived from the old smoke suite.

The delegation probe completes 256 child sessions, validates every journal and
checks descriptors at several points. Completed children must release their file
handles while the parent retains journal ownership. A separate blocked-child run
checks SIGINT cleanup. RSS is measured, without a fixed memory threshold.

See [the rewrite measurements](../performance/backend-rewrite-2026-09-26.md) for
sample sizes, build identity, comparison results and limits. Session listing,
search and resume have behavior tests; no current large-corpus latency claim is
made for them.

## TUI render budget

The release resize contract requires p95 below the 8.333 ms budget for 120 Hz. Deterministic
tests also cover startup, overlays, permissions, replay, Unicode geometry, and selection.
`scripts/measure-tui-performance.py` measures complete render/diff/ANSI encoding, CPU time,
resident memory, and terminal bytes across synthetic workloads. `scripts/measure-tui-runtime.py`
measures the production event loop and writer through a real PTY, including a slow reader.
See [the fluidity measurements](../performance/tui-fluidity-2026-09-13.md) for before/after data,
workload sizes, commands, and the distinction between PTY throughput and physical display refresh.

## Binary size budget

There is no enforced binary-size gate. Do not claim a size target without a
measurement of the built binary and an artifact tied to that build.

## Perf lane

`scripts/test-lanes.sh perf` runs the preserved release performance tests, builds
the CLI, then runs the local streaming and delegation probes. It writes commands,
exit statuses, output and JSON samples beneath the chosen lane artifact directory.
A failed measurement or failed output/journal assertion fails the lane.

Performance claims must link to measured samples and identify the build. Retained
historical reports describe their original revision; they are not current gates.
The old resume-plan timing target and artifact-freshness framework were removed
with the backend tests they owned.

## Failed budgets

Budgets are checked against current commands and artifacts. Do not add JSON baselines or allowlists that grandfather old measurements. When a budget fails, investigate the implementation and revise unsupported claims.
Do not change the threshold merely to accept the result.

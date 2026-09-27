# Backend rewrite measurements

These measurements use the final replacement backend and unchanged terminal
source. They cover local workloads, not model latency or physical display refresh.
The [sample artifact](backend-rewrite-2026-09-26.json) records every backend sample,
renderer medians, compiler, platform, source digest and binary digests.

## Local streaming comparison

Both executables were built with `cargo build --release -p harness --bin harness`
on the same machine and compiler. The reference is commit
`3db4029fd9f816703590593a297203913cc30860`. Five fresh processes per binary used an
isolated workspace and loopback HTTP/SSE provider, waited idle for one second,
then consumed 5,000 text fragments. Each sample verified the exact 25,001 output
bytes, a completed journal and the absence of durable provider fragments.

| Metric | Original median | Rewrite median |
| --- | ---: | ---: |
| Launch to provider request | 76.69 ms | 8.37 ms |
| First output after releasing the provider | 0.96 ms | 1.03 ms |
| Stream, journal commit and exit | 96.86 ms | 19.30 ms |
| User CPU | 168.12 ms | 23.10 ms |
| System CPU | 48.53 ms | 4.97 ms |
| Peak resident memory | 69.23 MiB | 26.03 MiB |
| Durable journal | 10 events; 56,580 bytes | 10 events; 56,157 bytes |

For this workload, startup fell 89%, stream completion fell
80%, and peak memory fell 62%.
Neither executable accumulated a CPU tick in any one-second idle window. The
10 ms clock resolution does not establish literally zero CPU use.

The batches ran without concurrent builds. Filesystem caches were warm. Startup
includes configuration, session creation and the provider request. Streaming
includes loopback transport, redaction, stdout delivery, coordinator events and
durable completion. The Python fixture's CPU and memory are excluded.

The replacement binary SHA-256 is
`a759c76842041e2d8c64ebcc7c0603f214fc606990021a7094c4a6b2b2707872`.
Its Rust/Cargo source digest is
`e6a8522eaabd727688631375b49ce26b6d7efbb60b0df282feab851eebbc5d89`.
The source digest identifies the measured replacement independently of its Git
commit; the reference commit identifies only the original backend.

## Completed children and cancellation

A separate local provider completed 256 sequential child sessions in
818 ms. The process retained 12 descriptors at all five observation
points, including after every child completed. RSS rose from
25.9 to 37.6 MiB.
The probe validated 257 journals, 3,850 root records and
5,955,360 durable bytes. A blocked-child run completed SIGINT cleanup
in 0.82 ms and verified both terminal journals.

An earlier debug measurement found two retained descriptors per completed child.
The coordinator now closes completed child writers while retaining parent
ownership through its kernel lock. The same 256-child workload stays at 12
descriptors, compared with growth from 12 to 524 before the fix.

These samples cover sequential children and one blocked-child cancellation.
They do not establish memory use for unlimited histories or arbitrary fanout.

## Preserved renderer

The existing renderer/diff/ANSI fixture completed 39 fresh processes: 13 scenarios,
three repetitions each, with 120 measured frames. No terminal source or fixture
changed. The table shows medians across repetitions.

| Scenario | History entries | p95 frame work | Resident memory after workload |
| --- | ---: | ---: | ---: |
| startup | 0 | 0.169 ms | 16.6 MiB |
| static | 100 | 0.143 ms | 18.6 MiB |
| static | 10000 | 0.389 ms | 223.5 MiB |
| scroll | 10000 | 0.613 ms | 223.8 MiB |
| typing | 10000 | 0.452 ms | 223.5 MiB |
| stream | 100 | 1.211 ms | 20.6 MiB |
| stream | 10000 | 6.180 ms | 252.0 MiB |
| code | 100 | 1.470 ms | 30.5 MiB |
| tool | 10000 | 4.778 ms | 251.5 MiB |
| hover | 10000 | 0.955 ms | 223.7 MiB |
| selection | 10000 | 0.842 ms | 236.7 MiB |
| resize | 10000 | 4.497 ms | 422.1 MiB |
| stream-events | 10000 | 7.056 ms | 260.7 MiB |

The largest median p95 was 7.056 ms, below the existing 8.333 ms frame-work
budget. This measures rendering, diffing and ANSI encoding; it does not measure
physical display refresh or prove a renderer speedup over the original backend.
The 10,000-entry stress fixtures disable normal transcript memory caps. Their
223–422 MiB resident sets remain a TUI cost.

## Reproduce

```bash
cargo build --release -p harness --bin harness
python3 scripts/measure-backend.py --binary target/release/harness \
  --output target/backend-performance.json --repetitions 5
python3 scripts/measure-delegation.py --binary target/release/harness \
  --output target/delegation-performance.json --children 256
XDG_CONFIG_HOME=/tmp/agent-harness-empty-test-config \
  python3 scripts/measure-tui-performance.py \
  --output target/tui-performance --repetitions 3 --frames 120
```

The backend probes use Python's standard library, Linux `/proc` and `wait4`.
They create isolated configuration and sessions and contact no public provider.
Failed output, journal or resource assertions reject the sample. Run benchmarks
serially; concurrent builds distort CPU, memory and latency measurements.

[Verification results](../architecture/backend-rewrite.md) cover behavior, native
processes, live services and preserved terminal failures separately.

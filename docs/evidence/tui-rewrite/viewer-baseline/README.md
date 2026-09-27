# Viewer release baseline

This baseline precedes further viewer changes. It compares the pinned original
`1bb0f989` with the accumulated rewrite at `706d00bf`; it does not isolate the
selection-layout change. Production source is unchanged by this commit.

The frozen fixture opens a recorded tool result through `AppState`, with 2,000
numbered lines containing CJK, combining marks and a ZWJ emoji. It measures 200
frames after ten warm-up frames at 120×40. Scrolling uses Ctrl-J/K; search alternates
`line_0050` and `line_00505`; resizing alternates 80 and 120 columns. Assertions
verify scrolling, search edits, wrapping, retained tail content and closing.
There is no provider or shell execution.

Frame samples include input handling, preparation, painting, Ratatui diffing and
ANSI encoding. They are not end-to-end terminal latency. `idle` forces redraws;
it does not measure runtime idle. Visible-content checkpoints run outside frame
timers but inside process CPU/allocation totals. Construction and cold opening
are reported separately. Generated input receipts are dropped before sampling.

All values below are medians of three timing runs. Allocation totals and peak
heap come from one separate glibc `memusage --no-timer` run per workload.

| Workload | p95 µs, original / before | p99 µs | CPU ms/frame | RSS KiB | Allocated bytes |
|---|---:|---:|---:|---:|---:|
| Forced redraw | 3250 / 1834 | 3456 / 1942 | 3.20 / 1.80 | 90692 / 59492 | 674941248 / 371551807 |
| Scroll | 3327 / 1928 | 3600 / 1958 | 3.30 / 1.85 | 90668 / 59772 | 678239612 / 374846891 |
| Search | 40216 / 8009 | 41199 / 8827 | 38.60 / 7.70 | 90764 / 59680 | 9932400481 / 847017594 |
| Resize | 71036 / 48020 | 86976 / 49511 | 57.25 / 35.60 | 90672 / 81548 | 16132309624 / 5858969857 |

Painting dominates steady viewer frames. Candidate median preparation during
resize is 33,233 µs; search input is 5,659.5 µs. `comparison.json` contains phase
medians and peak heap. Output bytes, final screen, tail and interaction checkpoints
match the original in every timing repetition.

`acceptance.json` freezes limits at 70% of the original p95, p99, CPU/frame, RSS
and allocated bytes for each workload. The current resize RSS misses that limit.
The earlier whole-rewrite limits remain unchanged and independently binding;
these additional diagnostics neither replace them nor establish completion.

## Reproduction and provenance

Copy the fixture and unchanged `support/rewrite_journey.rs` into the pinned
reference checkout. Give this opt-in test the same `profile.perf` resource timeout
as the candidate. Run serially, without competing builds or tests:

```sh
python3 scripts/measure-tui-rewrite.py --root /home/urbanbreach/.codex/worktrees/tui-reference/agent-harness --viewer-lines 2000 --output /tmp/viewer-reference --allocations
python3 scripts/measure-tui-rewrite.py --viewer-lines 2000 --output /tmp/viewer-before --allocations
```

`provenance.json` records commits, identical fixture hashes, driver hash and both
release binary hashes. Raw JSON samples and nextest/memusage logs are gzip files
under `reference` and `before`; summaries include platform and Rust versions.
`files.json` hashes the evidence. Reference production sources and Cargo.lock
remain at the pinned commit.

The first reference resize allocation trace hit the generic 20-second nextest
timeout. Its failure is retained. The new opt-in test now shares the existing
300-second resource-fixture timeout, in both checkouts. Only the failed allocation
trace was retried. The reference resize summary was recovered from the three
already successful timing files using the driver's median formula, then checked
against those files. No numerical acceptance threshold changed.

The fixture passes scoped Clippy, formatting and suite gates. A separate
1,000-line, 101-frame search run verifies odd-frame behavior; it is not a baseline
sample. Independent review corrected ineffective initial scroll/resize stimuli,
query assertions and retained fixture input before the final builds and samples.
No new visual, PTY, live-provider or whole-rewrite completion claim is made here.

# Unreachable post-run controller

The predecessor is `410779f3818e97b91efbcaa24421fcd581177a4c`.
This removes 133 production lines and three private state fields. No public
contract, backend code, rendered content or intended interaction changes.

`AppState::lifecycle_shell_state` returns only None or Startup, so
`post_run_handoff_visible` is always false. The removed selection/execution
methods were reachable only through that guard in the key handler. Their private
selection field has no other consumer. `continued_post_run_handoff_active` was
never set true and was read only in that unreachable branch;
`continued_live_reopen_surface_active` was written but never read. The equivalent
mouse predicate no longer checks the always-false query.

Public post-run enums and queries remain. The layout's `post_run_card` tokens
still size the live empty state, and the replay failure formatter still runs.
Reopen metadata and the active startup launcher remain unchanged.

## Verification

The existing completion/failure tests and pinned full-frame comparisons pass on
the predecessor. A temporary mutation that returns PostRun after a terminal event
makes both completion/failure checks fail; `red.patch.gz` and `red.log` preserve
that check. The mutation was reverted before removing the controller. No test
source or snapshot expectations changed.

- All 1,668 deterministic TUI tests pass, including the pinned frame and intent
  comparisons; seven gated tests are excluded from that run.
- All seven explicitly gated P0-04/P1-04 PTY tests pass.
- TUI Clippy, workspace compilation, formatting and suite gates pass.

`checks.json` records the exact commands, exit statuses and corresponding logs.
Run the candidate checks from the repository root. To reproduce the mutation,
apply the decompressed `red.patch.gz` in a disposable checkout of the predecessor,
run its two named tests, then revert the patch. `sources.json` identifies the
seven changed source files; `source.patch.gz` records the full implementation
diff. `files.json` hashes the evidence.

This is unreachable-code removal, with no new resource or latency improvement
claim. The all-TUI suite compares against the pinned original; new browser,
release benchmark and full-workspace test runs were not needed for this bounded
removal. The preceding reader slice records the outstanding CLI fixture failures.
Whole TUI source is 166,914 lines, only 8.23% below the original 181,882.
The remaining state engine, source and resource targets, non-Linux verification,
startup cadence and final rewrite review remain unfinished.

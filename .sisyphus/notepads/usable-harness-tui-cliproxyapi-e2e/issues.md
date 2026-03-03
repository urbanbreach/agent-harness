# Issues and Blockers

## Active Issues
None yet.

## Resolved Issues
None yet.

- 2026-03-03: Reported `Rect` indexing compile errors in `crates/harness-tui/src/ui.rs` could not be reproduced in current tree; file already uses non-indexed `Rect` flow and crate builds cleanly.
- 2026-03-03: `harness tui --config ...` default interactive mode initially failed with `event sequence mismatch` due run-id collisions against persisted sessions; resolved by using a unique run_id override for non-deterministic interactive runs.

## Security Notes
- Must redact secrets from JSONL/snapshots/artifacts
- API key patterns must be scrubbed
- No raw HTTP headers/bodies in logs

- 2026-03-03: PTY diff snapshot hash was initially unstable because focus region included temp-session absolute paths in `diff artifact missing` panel; resolved by narrowing focus capture to the marker line only (`anchored_exact`, 1-row region).
- 2026-03-03: Interactive PTY process did not exit reliably via `q` in this test path; resolved test teardown by explicit child termination after checkpoint capture.

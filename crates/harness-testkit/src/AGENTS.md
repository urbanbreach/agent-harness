# Test support

`workspace.rs` provides isolated temporary workspaces. `lib.rs` re-exports the
compatibility assertion helper used by terminal fixtures.

Keep shared helpers small and use them only when multiple callers need them.
Scripted providers live in `harness-providers::mock`; coordinator fixtures live
beside their behavioral tests. Do not recreate a second simulation runtime.

Never read or modify ambient user configuration. Use explicit temporary paths,
clocks, and notifications. Keep native and terminal checks opt-in, and preserve
the existing terminal support files.

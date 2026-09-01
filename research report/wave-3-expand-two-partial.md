# EXPAND wave two — partial journal

## Settled leads

### Dashboard anchor

Harness has the data model and capture primitive for exact return-state restoration, but dashboard entry hard-codes `transcript_anchor: None` and exit restores only focus. Populate and apply the anchor. Grok preserves pane-owned offset across an `ActiveView` flip, so no direct Grok capture port is required.

### Fold navigation

Pin-reserve is the surviving divergence. Harness derives/clamps page-flip state per frame; Grok persists reserve state across prompt/turn transitions. Do not port `wrap_restore.rs`: it owns terminal-mode escape restoration, not scrollback behavior.

### Rich copy and selection

Hyperlink propagation is production-integrated in Grok but orphaned in Harness. Wire Harness's existing OSC-8 encoder through markdown/table/selection output. The earlier `TableCopyMeta` name match was misleading: the two names represent different concerns.

### Command runtime

Add Grok's `takes_args × args_required` completeness model to Harness's static command metadata. Preserve the static deterministic registry; do not add dynamic ACP command replacement without an actual provider source and collision contract.

### Composer routing

Both systems implement running-turn interject as cancel-then-send. Harness separates interrupt and submission; Grok exposes one send-now path. The earlier claim that Harness loses a `SendNow` promise on `Cancelling → Idle` was not proved and is excluded.

### Scheduler and glyphs

Frame cadence, pulse cadence, and reduced-motion behavior already match. The only bounded polish item is routing every live-status rail through Harness's existing `GlyphMode` fallback inventory.

### Theme runtime

Terminal-native Basic capping and quantization already match. `theme_role_color` has no live consumer, so token transfer based on that helper is excluded. Preserve Harness colors and branding.

### Scroll pipeline

Both trees share a three-cell minimum thumb and follow-to-bottom semantics. Keep the supported follow-mode dimming transfer. Do not claim coast-budget parity as a simple patch: the coast implementation is isolated to a different Grok input layer and would add a clock/drain redesign.

### Resize and pin reserve

Harness rewrap anchors preserve `display_column`, a stronger contract than Grok's row-only anchor. Do not port Grok `ModeTracker`; it owns child-induced terminal modes. Persistent pin reserve remains conditional on a demonstrated page-flip bottom-pose defect.

### Disconnect runtime

Harness provider attempts retry internally, but the disconnected live-update path is not re-entrant. Its “Reconnecting…” copy is therefore false. Grok owns explicit disconnected, reconnecting, reconnected, and reloading transitions.

### PTY ownership

Add emulator-grade behavioral assertions to the existing Harness PTY owner (`crates/harness-tui/tests/pty_e2e.rs` through `tests/support/pty_e2e_impl.rs`). TestBackend snapshots and a parser that never returns query replies do not prove full VT behavior.

## Current convergence

- Eleven of eleven research leads are settled.
- Broad claims about scheduler drift, dynamic commands, theme quantization, coast parity, and anchor weakness were narrowed or rejected.
- Every emitted lead was investigated, deduplicated, conditionalized, or dead-ended.
- No genuine third-wave lead remains. The verifier is the final convergence gate.

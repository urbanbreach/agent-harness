# Local terminal-reader patch

This is Crossterm 0.29.0 from crates.io. `UPSTREAM.json` records the published
archive checksum, revision and original file hashes. The license and upstream
source layout are retained, including upstream files over 500 lines. This
third-party maintenance cost is separate from TUI source reduction.

The Unix `use-dev-tty` adapter exposes its existing wake pipe through
`poll_waker` and `poll_blocking`. The TUI owns and joins its reader thread;
it never constructs `EventStream` or starts that type's worker. The extra
feature is enabled only for Unix.

The source reads once per readiness notification, then returns to polling.
An incomplete UTF-8 sequence or paste can therefore wait for more input while
remaining interruptible. Wake readiness takes priority; buffered input and
readable bytes survive a simultaneous hangup. EOF and descriptor errors fail
instead of spinning. Interrupted polling preserves filtered protocol replies.
Wake-pipe allocation precedes SIGWINCH registration so failed construction
cannot leave that registration behind.

The root Cargo patch also affects other users of this dependency, including
CLI terminal queries. Parsing, output commands and raw-mode settings retain
their upstream implementations. Windows retains its prior feature set and
reader path. Linux PTY and xterm evidence lives in
`docs/evidence/tui-rewrite/reader-wake`; other systems remain unverified.

Keep this patch narrow when updating Crossterm. Remove the local API if an
upstream owned, interruptible reader provides the same guarantees. Re-run the
PTY shutdown/restoration checks, ordered input burst and browser latency
comparisons before replacing it.

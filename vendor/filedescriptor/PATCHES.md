# Local timeout correction

This is the published `filedescriptor` 0.8.3 source, under its original MIT
license. `UPSTREAM.json` records the upstream revision and the hashes of the
unmodified files. Cargo uses this copy through the root `[patch.crates-io]`.

The Unix `poll` and Windows `WSAPoll` paths now round positive durations up to
integer milliseconds and clamp to `c_int::MAX`. Zero remains nonblocking and
`None` remains an infinite wait. macOS's separate `select` path is unchanged.

Crossterm's level-triggered reader retries until its deadline. The original
conversion rounded its final fractional millisecond down to zero, causing a
busy loop even with an idle terminal. Keep the level-triggered reader: the
default Mio reader can leave input unread after a large burst.

The source changes are the shared conversion, its two callers, one boundary
table test, and two explicit inferred lifetimes to satisfy current compiler
warnings. Run the test with:

```sh
cargo nextest run --manifest-path vendor/filedescriptor/Cargo.toml
```

This adds 1,563 lines of upstream Rust to maintain separately from the TUI
rewrite, plus the local correction and test. Upstream platform files retain
their original layout. Remove the vendor patch when an upstream release fixes
the conversion; do not edit Cargo's registry cache.

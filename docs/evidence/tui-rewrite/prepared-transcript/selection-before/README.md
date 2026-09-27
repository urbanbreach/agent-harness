This is the intermediate prepared-layout implementation immediately before compact selection rows, not the pinned original rewrite reference. Apply `source.patch.gz` to `f24e8d6c`, and add `ui_transcript_frame.rs.gz` at `crates/harness-tui/src/ui_transcript_frame.rs`.

The single release sample uses 1,000 history entries, 100 drag/release/Escape cycles, and four paints per cycle. The parent README records the command. This baseline isolates the selection replacement; it is not whole-rewrite acceptance evidence.

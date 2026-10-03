# Current settings registry regression

`rewrite_reference_test` retains all 543 complete-frame comparisons. The original
Harness oracle, `tui-reference.cells.jsonl`, and its original-source-only recording
guard are unchanged. The documented R8 correction still applies only to the two
original detached-history/append frames.

The intentionally expanded registry has 59 runtime rows rather than 45.
Consequently, eight complete frames use a separate current contract:
`dialog-settings` and `selected-settings` at 40x24, 80x24, 120x40 and 160x50.
They come from `tui-current-settings-registry-20261003.cells.jsonl`, with a
matching `.identity.json`. The remaining 535 frames retain the original oracle.
This is Harness regression evidence, not Grok parity.

The identity pins the whole fixture, ordered registry definitions, editor kinds,
and explicit frame inventory. A changed registry or corrupted fixture fails
closed. Rendering is checked against every expected cell, so unrelated source
edits do not require a new fixture identity. There is no
candidate recording switch. Independently derived checks verify row order,
read-only binding, selection, visible label/style geometry, scrollbar arithmetic,
cursor, input deltas and intent absence before full-frame comparison.

Every cell, including blank cells and all six stored cell fields, is compared.
The existing journey also runs corruption controls for symbols, styles, counts,
cursor, input, intent, editor kinds and registry binding. No cells are patched,
masked or normalized to create the current expectation.

Run the owning target with:

```sh
cargo nextest run --profile ci --all-features -p harness-tui --test rewrite_reference_test
```

`HARNESS_TUI_REFERENCE_FRAMES` exports all complete journeys, source/binary
identity and production Crossterm streams. Settings `.settled.ansi` streams are
captured before terminal teardown so screenshots authenticate the actual cursor
state. The ordinary `.ansi` streams retain teardown output. Neither export writes
an expectation fixture.

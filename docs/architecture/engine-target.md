# Engine target

The target keeps one coordinator-owned append/lifecycle authority and replaces overlapping
session/context/compaction interpretations with one canonical durable session path.

## Interactive TUI flow

```text
TUI composer → coordinator → ResolvedModelLimits + RequestBudget → CanonicalSession
→ provider request/ephemeral deltas → coordinator commit → canonical read model → TUI
```

## Headless flow

```text
harness run → coordinator → ResolvedModelLimits + RequestBudget → CanonicalSession
→ provider request/ephemeral deltas → coordinator commit → canonical read model → inspect/list
```

| Decision | Target responsibility |
|---|---|
| Keep | coordinator as the sole append, scheduling, permission, hook, compaction, and lifecycle authority |
| Consolidate | model-limit provenance, request budget, session history, provider context, and compaction pipeline |
| Move | CLI, TUI, session tools, and export consumers onto replay-derived canonical views |
| Disable | mutation of legacy sessions and any duplicate active compaction path |
| Delete | superseded checkpoints, duplicate reducers, deprecated active compaction vocabulary, and unreachable context paths after cutover |

The target preserves append-only events, side-effect-free replay, provider-specific lowering in
adapters, and ephemeral streaming separate from durable semantic history.

## Canonical model-limit contract

`ResolvedModelLimits` owns the resolved context window, maximum provider-visible input, maximum
output, and field-level provenance. Configuration and catalog boundaries reject partial, zero, or
impossible triples. Fully absent custom-model limits remain explicit unknowns, and family detection
never creates numeric limits. Variant resolution changes only explicitly overridden fields.

Request-budget math is deliberately separate: M02 establishes authoritative inputs and their
meaning, while M03 will derive per-request budgets from them.

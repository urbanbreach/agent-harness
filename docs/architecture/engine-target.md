# Engine architecture boundary

The engine keeps one coordinator-owned append and lifecycle authority. Durable semantic history,
live provider presentation, and compatibility decoding have separate contracts.

## Durable semantic history

The coordinator appends V1 envelopes to `events.jsonl` for lifecycle barriers, user messages,
completed assistant messages, tool activity, permissions, and other semantic facts. A new assistant
completion is self-contained in `AssistantMessageFinished`: ordered sanitized reasoning, text,
completed tool intents, provider provenance, and optional assistant message metadata are committed
together before tool execution.

Provider fragments are bounded, lossy, non-replayable runtime events. Text, reasoning, and partial
tool input use a 1024-item broadcast channel for connected runtime subscribers. Lag can drop those
fragments. They aren't appended to JSONL, and replay never returns them. If provider transport ends
without a final assistant commit, its fragments don't become canonical assistant history.

The legacy `EventV1` variants `ProviderStreamDelta` and `ProviderReasoningDelta` remain decode-only.
Compatibility readers still accept them, so old partial logs remain readable. When an old log has
fragments but no final assistant content, canonical projection preserves the available partial
content and reports a structured warning.

## Read model boundary

`CanonicalSession` is the typed canonical read domain for session, run, entry, turn,
provider-request, and tool-call identity. `LegacyEventLogAdapter` projects borrowed V1 history into
that domain without writing files or executing runtime work. For self-contained assistant commits,
the committed parts replace any earlier compatibility fragments.

The runtime still contains V1-specific provider-context, resume, transcript, session, export,
catalog, lineage, and compaction projections. Later projection consolidation and deletion are not
yet complete.

## Runtime data flow

```text
provider transport
  -> bounded live fragments -> connected runtime subscribers
  -> provider finish
  -> self-contained AssistantMessageFinished
  -> append-only events.jsonl
  -> replay-derived read models
```

| Property | Current responsibility |
|---|---|
| Append and lifecycle authority | Coordinator |
| Durable assistant content | `AssistantMessageFinished.parts` and `AssistantMessageFinished.provenance` |
| Live provider presentation | Bounded runtime event broadcast |
| Old delta history | Decode-only V1 compatibility paths |
| Canonical typed session reads | `CanonicalSession` through `LegacyEventLogAdapter` |
| Provider context and product projections | Existing replay-derived V1 consumers, not yet consolidated |

## Canonical model-limit contract

`ResolvedModelLimits` owns the resolved context window, maximum provider-visible input, maximum
output, and field-level provenance. Configuration and catalog boundaries reject partial, zero, or
impossible triples. Fully absent custom-model limits remain explicit unknowns, and family detection
never creates numeric limits. Variant resolution changes only explicitly overridden fields.

Request-budget math is separate from limit resolution. `RequestBudget` derives per-request
allowances from the resolved limits and the request's system, tool, attachment, framing, output,
and compaction inputs.

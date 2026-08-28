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

For each journal load or settled durable-event batch, the runtime constructs one authoritative composed `CanonicalSessionProjection` facade per journal load or settlement. The facade composes focused pure reducers; it is neither a monolithic reducer nor a single physical pass. Provider continuation, restart, replay, export, catalog inspection, and settled TUI state consume its typed views. The TUI retains only ephemeral overlays and presentation enrichment; it does not define durable session semantics.

The facade is the active read boundary, while legacy conversion is deliberately narrow: deprecated
`EventV1` shapes are matched only inside the read-only `session::legacy` compatibility boundary.
The shipped `EventV1` variant count remains 39; this preserves old journal decoding without
allowing a legacy writer or a second durable source of truth.

## G007 canonical provider continuation boundary

The canonical provider view is the single semantic input for a continuation. It carries the owning
agent/session identity, persisted active leaf and watermark, ordered protocol-safe entries,
complete tool-call/result pairs, the latest compaction summary, typed attachments, usage boundaries,
pending prompt, and the redacted runtime selection. The selection preserves provider/model,
variant, reasoning and thinking settings, resolved limits, and a profile/tool-shape digest. Live
turn start and restart/reopen recovery use the same provider continuation lowerer; a current
profile or tool-shape mismatch fails closed before provider dispatch.

The migration work is complete for the shipped boundary described here. Focused reducers remain
intentionally separate so their independent error contracts and tests remain explicit. No
provider-ready request, raw prompt/tool schema, secret, or hidden reasoning is persisted.

## G006 Compaction V2 boundary

The current target dispositions are explicit and do not claim later milestone work:

| Disposition | G006 contract |
|---|---|
| Keep | one coordinator-owned `prepare -> generate -> validate -> commit` pipeline; one `SessionCompaction` success event; typed canonical active-path entries; replay-derived restart context |
| Consolidate | manual, pre-prompt, and overflow triggers; shared request-budget accounting; live and reopened provider-context projection |
| Move | provider-context restoration and consumer projections onto the committed typed event path after equivalence evidence |
| Disable | trigger-specific compaction bypasses, checkpoint artifact writes, and any second success writer |
| Delete | no G006 deletion of legacy event variants/readers; compatibility deletion belongs to G010 after migration evidence |

Failure is atomic: empty, failing, cancelled, stale, malformed, or non-fitting summary generation
leaves the previous boundary active and appends no replacement success event. The optional typed
`SessionCompactionEvent` fields (`first_kept_entry_id`, `tokens_after`, `summary_usage`, provider/model
provenance, file state, and current intent) are serde-defaulted so old logs remain readable.

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

### Interactive TUI flow

The TUI submits prompts and `/compact` through the coordinator-owned V2 pipeline. It observes live
fragments for presentation, then reads the committed `SessionCompaction` event and replay-derived
provider context; it does not write a checkpoint artifact or append a second success event.

### Headless flow

The headless prompt/run surfaces use the same coordinator, request-budget snapshot, typed active-path
cut, and restart reconstruction. Manual, pre-prompt, and bounded overflow compaction therefore share
the same durable event shape and failure-atomic commit boundary.

## Canonical model-limit contract

`ResolvedModelLimits` owns the resolved context window, maximum provider-visible input, maximum
output, and field-level provenance. Configuration and catalog boundaries reject partial, zero, or
impossible triples. Fully absent custom-model limits remain explicit unknowns, and family detection
never creates numeric limits. Variant resolution changes only explicitly overridden fields.

Request-budget math is separate from limit resolution. `RequestBudget` derives per-request
allowances from the resolved limits and the request's system, tool, attachment, framing, output,
and compaction inputs.

## G008-G012 final ownership matrix

`CanonicalSessionProjection` is the durable read owner. It composes the canonical semantic
session, conversation, resume plan, run summary, timeline, transcript, tasks, permissions, and
lineage once at a load or settlement boundary for provider continuation, restart, replay/export,
catalog inspection, and settled TUI state. Provider text, reasoning, and tool-input fragments are
an **ephemeral TUI overlay**; the semantic assistant commit replaces those fragments exactly once.

| Surface | Status | Shipped behavior |
|---|---|---|
| canonical durable projection and provider-context builder | supported | one authoritative composed facade is constructed at each journal load or settlement; focused pure reducers remain deliberately separate |
| Compaction V2 | supported | the coordinator appends one atomic `SessionCompaction`; no checkpoint artifact or second success event is written |
| bounded history index | supported | a history-index row is updated after each successful durable commit; unchanged warm lists use the index and `warm journal opens: 0` |
| `sessions search`, `sessions list`, and `sessions rebuild-index` | supported | cursors include timestamp, run id, and run-directory bytes; an invalid or stale cursor fails closed, and rebuild reports scan/open counts and recovery reason |
| live provider fragments | experimental | presentation-only TUI state, never durable session truth; No live-provider assertion is made without configured credentials and a live signoff |
| `CompactionRequested`, `CompactionWritten`, `CompactionApplied`, `CompactionFailed` | compatibility-only | serde/replay decoding for shipped logs only inside the read-only `session::legacy` compatibility boundary; production never appends them |
| checkpoint artifact loader/writer/copy path and detached context builders | removed | no active production reachability |

Legacy logs remain read-only compatibility input. Replay and continuation validate source
`events.jsonl`; index rows locate and summarize sessions but never become continuation authority.

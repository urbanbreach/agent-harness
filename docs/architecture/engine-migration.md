# Engine migration

Migration starts from baseline commit `060ee1fd` and is intentionally serial: limits and budget,
then canonical session semantics, then compaction, consumers, bounded indexing, and deletion.

| Stage | Baseline | Target | Migration disposition |
|---|---|---|---|
| model limits/budget | distributed policy/context calculations | one typed provenance record and budget | consolidate before consumer changes |
| durable session | overlapping event/projection interpretations | one canonical session and typed identities | migrate readers before writers |
| compaction | legacy and active variants coexist | one atomic, restart-safe pipeline | disable old active path, then Delete it |
| consumers | CLI/TUI/tools independently reconstruct history | canonical replay-derived views | Move each consumer after equivalence proof |
| cleanup | SIZE_OK and overlap counts are baseline facts | net-negative overlap with evidence | Delete only after green owner evidence |

Legacy logs remain readable for list, search, inspect, replay, and export. Their mutation paths
fail as read-only until a separate migration is explicitly implemented. Each stage records an
`engine-metrics-v1` artifact against the same baseline so reductions are comparable rather than
silently rebased.

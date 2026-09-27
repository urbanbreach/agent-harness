# Core implementation

| Work | Source |
| --- | --- |
| Actor and lifecycle | `coord/runtime.rs`, `coord/lifecycle.rs` |
| Provider turns | `coord/turn.rs`, `coord/turn/`, `coord/streaming.rs` |
| Permissions and tools | `coord/tools.rs`, `coord/grants.rs`, `coord/edit_paths.rs` |
| Child ownership and journals | `coord/children.rs`, `coord/child_journals.rs` |
| Context and compaction | `coord/context.rs`, `coord/history.rs`, `coord/compaction.rs` |
| Resume and rewind | `coord/resume.rs`, `coord/workspace/` |
| Configuration | `config/loader.rs`, `config/discovery.rs`, `config/normalize.rs` |
| Journal and recovery | `store.rs`, `store/`, `crash_recovery.rs` |
| Session branches | `session_lineage.rs`, `session_lineage/` |

Keep authority in the actor. Append the owning transition before returning a
successful result. Storage failure cancels active work and rejects new work.
Historical reconstruction uses pure event paths; only new operations run hooks.

Retain only active conversation buffers. Immutable profiles are shared. Bound
queues, process output, HTTP bodies, and artifact reads at their input boundaries.
Avoid per-fragment history scans and duplicate caches.

Compaction must keep complete tool-call/result pairs and preserve retained turn
identity. Resume honors recorded model selection with current policy. Deny rules
always override grants. Unknown limits and missing platform capabilities cannot
be treated as success.

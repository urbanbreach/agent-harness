# Backend rewrite

The backend rewrite is complete. The TUI source, adapters,
fixtures, assets and terminal QA owners remain unchanged.

## Scope and size

The reference is commit `3db4029fd9f816703590593a297203913cc30860`.
The first mutation deleted 857 backend source, test and fixture files. Eight shared
terminal-support and prompt/TUI preflight files were later identified as protected
TUI dependencies and restored byte-for-byte. The corrected deletion covers 849
files and 260,204 Rust lines. No backend implementation was restored.

The protected manifest contains 853 files. Its SHA-256 check found no changes.
The replacement contains 65,364 backend Rust lines in 339 files, including tests;
the largest file has 494 lines. This is roughly a 75% reduction from the deleted
backend. Compatibility modules retain only the public contracts the TUI uses.

## What runs where

[Runtime architecture](architecture.md) describes the coordinator and worker loop.
[Session storage](sessions-and-replay.md) covers replay, recovery, child journals,
compaction and forks. Operator and tool contracts are documented here:

| Surface | Guide |
| --- | --- |
| Prompt streaming, selection, resume and fork | [Prompt commands](../operations/prompt.md) |
| Authentication and provider protocols | [Providers](../operations/providers.md) |
| Session inspection, rewind, export and archives | [Sessions](../operations/sessions.md) |
| Child roles, permissions, continuation and background results | [Tasks](../operations/generic-agent-and-tasks.md) |
| Rule order, sensitive reads, external paths and remembered approval | [Permissions](../permissions/permissions.md) |
| Native files, shell, MCP, LSP and remote tools | [Tool catalog](../tools/native-tool-catalog.md) |
| Memory, code index, attribution, in-session input queues and worktrees | [Workspace data](../operations/workspace-data.md) |
| Lifecycle commands and vetoes | [Hooks](../operations/hooks.md) |
| Configuration inspection and schemas | [Inspection](../operations/inspection.md) |
| Stdio peer diagnostics | [Operators](../operations/operators.md) |

The rewrite retains existing working behavior. It does not add previously inert
workers for memory, cron, teams or plugin execution. Whole-workspace shell snapshots
and patch moves had no working production path in the reference. PDF bytes are
retained as private artifacts; PDF text extraction is not a supported provider input.

## Resource behavior

Queues, active tools, provider work, tool catalogs, subprocess output, attachments
and remote responses have explicit limits. Idle work waits on channels or permits.
Completed child contexts are released. Completed child journal writers also close;
the parent's kernel lock preserves ownership until shutdown.

A local 256-child debug probe exposed two retained descriptors per completed child.
After the fix, the process retained 12 descriptors at every measured point, instead
of growing from 12 to 524. Its SIGINT check completed cancellation in about 2 ms.
The probe validates every child journal and terminal record before accepting a
measurement. Release measurements are recorded separately in
[the performance report](../performance/backend-rewrite-2026-09-26.md).

No new cache was added for image parsing or digest verification without a measured
benefit. The TUI retains its existing rendering and history-memory costs.

## Verification

Use the [testing guide](../testing/testing.md) for repeatable commands.
Meaningful behavior changes were developed with failing assertions followed by
nextest checks. Existing behavior tests were extended before adding new ones.
The old backend test framework, simulation matrix and stale lane references were
removed. Shared terminal tests remain intact.

The latest full deterministic run completed 2,161 tests: 2,155 passed, six failed,
and seven were skipped. All six failures also occur on the reference commit:

- Two TUI snapshots omit the expected footer context label. Their generated diffs
  are identical with the original and rewritten backend.
- Two legacy model-selection tests expect retired GPT-5.4 Mini behavior; each runs
  in two preserved binaries. Both reproduce against the reference backend.

No snapshots were approved or changed. Configuration-sensitive checks use an empty
`XDG_CONFIG_HOME`; the machine's existing partial global configuration also breaks
the original startup tests and was left unchanged.

The native backend lane passes 13 checks across process cleanup, MCP stdio, Git,
filesystem confinement, reflinks, formatting, structural edits, language services
and binary replacement. The recorded CLI/TUI journey also passes: startup, prompt,
permission, file edit, resume and quit. Static backend gates and strict Clippy pass.
The separate branding gate still flags source-reference names in historical plan 001; the same
failure reproduces in the reference checkout.

A live Codex turn returned `PONG` using the existing stored credential and produced
a completed journal without provider fragments. Public MCP discovery, web search
and code search also passed. Fresh OAuth login was covered by local protocol
fixtures; no new live login was performed.

Nine preserved PTY/render failures also reproduce against the reference backend.
The clean browser capture run passed 12 of 13 cases: settings dialogs, startup
reveal, responsive states and Basic/ASCII mode passed. The 80×24 smoke case misses
the draft marker covered by the Commands overlay; the original executable fails
the same assertion. Captures include source/binary provenance and process cleanup
receipts. Representative screenshots were also inspected.

All 39 renderer measurements and the three existing release performance contracts
passed. The largest scenario median p95 was 7.056 ms. The local backend workload
used 62% less peak memory and completed streaming 80% faster than the reference.
These results do not measure physical display refresh. Full samples and limits
are in [the performance report](../performance/backend-rewrite-2026-09-26.md).

## Independent review

An independent agent checked the source, protected hashes, deletion scope, test
receipts, original feature exclusions and measurement claims. It found three
backend defects: unredacted live output, unbounded reasoning bytes and a repetition
counter carried between runs. All were corrected with failing-then-passing checks.
The streaming fix also exposed duplicate CLI buffering; removing that buffer
restored the existing early-output handshake and reduced display code.

The reviewer checked the refreshed measurements and browser provenance against
the corrected source and found no remaining backend blocker. The preserved TUI
failures above remain outside this rewrite; this report does not claim every test
is green.

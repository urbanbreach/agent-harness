# Plans and audits

| Audit | Source commit | Status |
|---|---|---|
| [Performance audit](performance-audit/README.md) | `9abbd54f` | Six candidates and scoped settlement optimization implemented; [A/B results](performance-audit/RESULTS.md) available |

The performance follow-up preserves the audit's durability and canonical-projection
constraints. Its results include fresh baseline comparisons, scoped checks, manual
terminal QA, and eight full-suite failures reproduced on the pristine baseline.

## Completed implementation plans

| Plan | Priority | Effort | Depends on | Status |
|---|---|---|---|---|
| [001: Give subagents independent role permissions](001-role-scoped-subagents.md) | P1 | M | None | Complete |
| [004: Evaluate file permissions against effective workspace targets](004-align-file-permissions-with-targets.md) | P1 | M | None | Complete |
| [005: Redact tool artifacts before writing them to disk](005-redact-persisted-tool-artifacts.md) | P1 | M | None | Complete |
| [006: Validate restore targets before changing workspace files](006-contain-workspace-restores.md) | P1 | M | 004 (Complete) | Complete |
| [007: Preserve existing file permissions during atomic edits](007-preserve-modes-on-atomic-edits.md) | P1 | S | None | Complete |
| [008: Preserve authored permission-rule order through configuration loading](008-preserve-authored-permission-order.md) | P1 | M | 002 (Complete) | Complete |
| [009: Keep malformed provider payloads out of errors and logs](009-remove-provider-payloads-from-errors.md) | P1 | S | None | Complete |
| [010: Reject malformed OAuth callback encoding without panicking](010-decode-oauth-callbacks-without-panics.md) | P1 | S | None | Complete |
| [011: Require a valid terminal outcome before completing provider streams](011-fail-incomplete-provider-streams.md) | P1 | M | 009 (Complete) | Complete |
| [012: Admit native tool execution through the coordinator scheduler](012-schedule-native-tools.md) | P1 | M | None | Complete ([#235](https://github.com/urbanbreach/agent-harness/issues/235)) |
| [013: Remap retained-history IDs when forking a compacted session](013-remap-fork-history-references.md) | P1 | M | None | Complete |
| [014: Preserve original bytes when a rewind operation fails](014-make-rewind-rollback-transactional.md) | P1 | M | 006 (Complete) | Complete ([#237](https://github.com/urbanbreach/agent-harness/issues/237)) |
| [015: Terminate MCP processes when initialization fails or is cancelled](015-clean-up-failed-mcp-startup.md) | P2 | S | None | Complete ([#238](https://github.com/urbanbreach/agent-harness/issues/238)) |
| [016: Cancel the HTTP reader when a provider stream is dropped](016-cancel-dropped-provider-streams.md) | P2 | S | None | Complete ([#239](https://github.com/urbanbreach/agent-harness/issues/239)) |
| [017: Contain archive traversal and honor session inclusion](017-contain-archive-traversal.md) | P1 | M | None | Complete ([#240](https://github.com/urbanbreach/agent-harness/issues/240)) |
| [018: Keep secret values out of QA scan diagnostics](018-keep-qa-secret-scans-secret-free.md) | P1 | S | None | Complete ([#241](https://github.com/urbanbreach/agent-harness/issues/241)) |
| [019: Remove encoded MCP media from durable tool results and support exports](019-strip-mcp-media-from-durable-state.md) | P1 | M | plan 005 | Complete ([#242](https://github.com/urbanbreach/agent-harness/issues/242)) |
| [020: Recover abandoned writer-recovery guards without racing another writer](020-recover-stale-recovery-guards.md) | P1 | M | None | Complete ([#243](https://github.com/urbanbreach/agent-harness/issues/243)) |
| [021: Enforce byte limits while capturing shell and MCP output](021-bound-tool-output-capture.md) | P2 | M | plan 015 | Complete ([#244](https://github.com/urbanbreach/agent-harness/issues/244)) |
| [022: Frame MCP SSE as bytes before decoding UTF-8](022-preserve-mcp-sse-framing.md) | P2 | M | plan 021 | Complete ([#245](https://github.com/urbanbreach/agent-harness/issues/245)) |
| [023: Preserve configured model selection when overriding thinking settings](023-preserve-thinking-override-model-selection.md) | P2 | S | None | Complete ([#246](https://github.com/urbanbreach/agent-harness/issues/246)) |
| [024: Preserve existing evidence during test-lane dry runs](024-preserve-evidence-during-test-lane-dry-runs.md) | P1 | S | None | Complete ([#247](https://github.com/urbanbreach/agent-harness/issues/247)) |
| [025: Return failure for CLI commands that have no implementation](025-fail-unsupported-cli-commands.md) | P2 | S | None | Complete ([#248](https://github.com/urbanbreach/agent-harness/issues/248)) |
| [026: Make live and native lanes select their opt-in test binaries](026-select-opt-in-test-lanes.md) | P2 | S | plan 024 | Complete ([#249](https://github.com/urbanbreach/agent-harness/issues/249)) |
| [027: Write nextest JUnit reports where CI collects them](027-align-nextest-junit-paths.md) | P2 | S | None | Complete ([#250](https://github.com/urbanbreach/agent-harness/issues/250)) |
| [028: Run the canonical performance evidence lane in GitLab CI](028-run-canonical-perf-ci-lane.md) | P2 | S | plan 027 | Complete ([#251](https://github.com/urbanbreach/agent-harness/issues/251)) |
| [029: Keep the mock TUI model picker offline](029-keep-mock-model-picker-offline.md) | P2 | S | None | Complete ([#252](https://github.com/urbanbreach/agent-harness/issues/252)) |
| [030: Keep child-task events from completing or hiding parent dashboard rows](030-isolate-dashboard-row-lifecycle.md) | P2 | M | None | Complete ([#253](https://github.com/urbanbreach/agent-harness/issues/253)) |
| [031: Enable advertised transcript review while a permission prompt is parked](031-enable-parked-permission-review.md) | P2 | M | None | Complete ([#254](https://github.com/urbanbreach/agent-harness/issues/254)) |
| [032: Remove unused reasoning and tool-input fragment archives](032-remove-unused-stream-fragment-archives.md) | P2 | S | None | Complete ([#255](https://github.com/urbanbreach/agent-harness/issues/255)) |
| [033: Load session lineage before TUI layout and navigation projection](033-keep-session-layout-free-of-io.md) | P2 | M | None | Complete ([#256](https://github.com/urbanbreach/agent-harness/issues/256)) |
| [034: Release each session journal before inspecting the next one](034-bound-session-inspection-memory.md) | P2 | M | None | Complete ([#257](https://github.com/urbanbreach/agent-harness/issues/257)) |
| [035: Yield Anthropic response events before the HTTP body ends](035-stream-anthropic-responses-incrementally.md) | P2 | M | plan 011; plan 016 | Complete ([#258](https://github.com/urbanbreach/agent-harness/issues/258)) |
| [036: Publish simulation evidence only after validation and secret scanning](036-publish-simulation-evidence-after-scan.md) | P1 | M | None | Complete ([#259](https://github.com/urbanbreach/agent-harness/issues/259)) |
| [037: Align operator documentation with active permissions and provider behavior](037-align-operator-docs-with-runtime.md) | P2 | S | plan 029 | Complete ([#260](https://github.com/urbanbreach/agent-harness/issues/260)) |

See the [2026-09-20 issue verification record](2026-09-20-issue-closeout.md) for independent review, attached commits and integrated checks.

Planned at `062e5ea8` on 2026-09-14 after comparing the local reference implementations. The user confirmed independent child-role permissions under shared
project restrictions. The plan keeps the existing profiles and task runtime;
custom role formats, team orchestration, and scheduler changes are out of scope.

Implementation and the approved fixture updates are complete. Scoped behavioral
checks, simulation and xterm.js proof pass. The original plan 001 delivery recorded 4,613 passes
and three independently reproduced baseline failures, plus the existing plan/index
branding failure. See that plan's historical delivery evidence; current results are
in the issue verification record above.

A subsequent user-requested follow-up enables bash, LSP, skill loading, and
configured MCP discovery for both research roles; their native edit and delegation
denies and the shared-policy ceiling remain. The current contract is in
[generic agent and tasks](../docs/operations/generic-agent-and-tasks.md).

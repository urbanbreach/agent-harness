# Agent Harness Milestone 1 — Tool-Aware Multi-Turn Loop + Structured Tool Calling + Orchestration + TUI Visibility

## TL;DR
> **Summary**: Add native structured tool-calling + a bounded multi-turn agent loop, tighten authorization (toolset/category derivation), expand high-value tools, and surface everything in the TUI with deterministic tests.
> **Deliverables**:
> - Provider abstraction supports tool defs + tool-call streaming events
> - OpenAI-compatible provider parses tool calls for **Responses** and **Chat Completions** streaming
> - Coordinator enforces toolset/category from `actor.agent_id` (ignores caller category), applies scheduler limits to tool tasks, and supports awaitable tool execution
> - Multi-turn tool-aware agent loop with strict guardrails + failure modes
> - Tool output persistence policy: capped event summaries + redacted artifacts for bulky outputs
> - Tool ecosystem expansion (hashline scan + file discovery/search basics)
> - TUI shows tool calls + results + spawned agents + permission lifecycle; PTY E2E + unit/integration tests updated
> **Effort**: Large
> **Parallel**: YES — 4 waves
> **Critical Path**: Provider API → OpenAI parsing → Coordinator tool await + scheduler → Multi-turn loop → TUI + PTY E2E

## Context
### Original Request
- Bring this Rust agent harness closer to established harnesses (OpenCode, pi-agent-rust) using `inspiration/` for inspiration only.
- Optimize next milestone for **Interactive CLI/TUI coding**.
- Parity priorities: **Multi-agent orchestration** + **Tool ecosystem expansion**.
- Compatibility: **Independent** (borrow ideas/UX; no OpenCode plugin-compat).
- Tests posture: **tests-after**.
- Extensions runtime: **defer** (design extension points only).

### Interview Summary
- No additional user tradeoffs required for Milestone 1; proceed with a focused, safety-first MVP.

### Metis Review (gaps addressed)
- Add explicit acceptance criteria for malformed streamed tool-call JSON fail-closed, permission wait deadlocks, cancellation/late results, multi tool calls in one turn, function-name ↔ tool-id mismatches, artifact redaction/caps, and scheduler enforcement for tool tasks.

## Work Objectives
### Core Objective
Enable an OpenCode-like interactive loop: model streams → emits structured tool calls → harness executes tools (permission-gated + logged) → reinjects results → repeats until done.

### Deliverables
1. **Provider Tool Calling Contract** (request + stream events) in `crates/harness-providers/`.
2. **OpenAI-compatible Structured Tool Calling** (Responses + ChatCompletions).
3. **Coordinator Safety/Correctness**:
   - Toolset/category derived from `actor.agent_id` (caller category ignored)
   - Tool execution uses Scheduler limits (tool concurrency enforced)
   - Awaitable tool execution (no polling JSONL)
   - Output persistence policy: capped summaries + artifacts
4. **Multi-turn tool-aware agent loop** with guardrails.
5. **Tool ecosystem** additions: `edit.hashline_scan` + minimal file discovery/search tools + improved `fs.read`.
6. **TUI visibility** for tool calls/results + delegation + permissions; PTY E2E updates.

### Definition of Done (agent-verifiable)
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features -- --test-threads=1`
- `HARNESS_DETERMINISTIC=1 HARNESS_DISABLE_ANIMATIONS=1 TZ=UTC LANG=C.UTF-8 LC_ALL=C.UTF-8 TERM=xterm-256color RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e -- --test-threads=1`
- New tests added for: tool-call parsing (chat+responses), multi-turn loop, permission wait/timeout, scheduler enforcement for tool tasks, toolset/category derivation, artifact redaction/caps, TUI state ingestion for tool calls.

### Must Have
- No “fake tool calls” encoded as text deltas.
- Tool calls only executed when (a) registered, (b) capability allowed, (c) toolset allows, (d) permission policy allows/asks.
- Loop is bounded (max iterations/tool calls) and fails closed on malformed tool args.
- Deterministic test runs remain deterministic.

### Must NOT Have (guardrails)
- Do NOT copy code from `inspiration/`.
- Do NOT trust caller-provided `category` for permission decisions.
- Do NOT bypass scheduler for tool tasks.
- Do NOT store unbounded tool outputs in JSONL events.

## Verification Strategy
> ZERO HUMAN INTERVENTION — all verification is agent-executed.
- Test decision: **tests-after** (existing unit + integration + PTY E2E + insta snapshots).
- QA policy: Every TODO includes agent-executed scenarios + concrete commands.
- Evidence: `.sisyphus/evidence/task-{N}-{slug}.{ext}` (executor writes artifacts).

## Execution Strategy
### Parallel Execution Waves
Wave 1 (Foundations): Provider contract + tool specs + coordinator auth derivation
Wave 2 (Provider + Mocks): OpenAI parsing (chat+responses) + MockProvider fixtures
Wave 3 (Coordinator runtime): tool await plumbing + scheduler gating for tools + artifact policy
Wave 4 (User surface): multi-turn loop integration + tools expansion + TUI + PTY E2E

### Dependency Matrix (full, all tasks)
| Task | Blocks | Blocked By |
|------|--------|-----------|
| 1 | 2,3,9 | - |
| 2 | 9 | 1 |
| 3 | 9 | 1 |
| 4 | 9,18 | 1 |
| 5 | 1,9 | - |
| 6 | 7,8,9,16,18 | - |
| 7 | 8,9 | 6 |
| 8 | 9,16 | 6,7 |
| 9 | 16,17,18 | 1,2,3,5,6,7,8 |
| 10 | 18 | 6,7 |
| 11 | 9,18 | 5 |
| 12 | 9,18 | 5 |
| 13 | 9,18 | 5 |
| 14 | 9,18 | 5 |
| 15 | 9,18 | 5 |
| 16 | 18 | 6,8,9 |
| 17 | 18 | 6,9 |
| 18 | - | 9,10,11,12,13,14,15,16,17 |

### Agent Dispatch Summary
- Wave 1: architecture / rust-core changes (unspecified-high)
- Wave 2: provider parsing + fixtures (unspecified-high)
- Wave 3: coordinator scheduling + artifact/redaction policy (ultrabrain)
- Wave 4: TUI + PTY E2E + tools (visual-engineering + unspecified-high)

## TODOs
> Implementation + Test = ONE task. Never separate.
> EVERY task MUST have: Agent Profile + Parallelization + QA Scenarios.

- [x] 1. Extend provider contract for structured tool calling (request + stream events)

  **What to do**:
  - Update `crates/harness-providers/src/lib.rs` to support structured tool calling:
    - Extend `CompletionMessage` with `name: Option<String>` and `tool_call_id: Option<String>`.
    - Add `ToolDef` (tool_id + provider function_name + description + parameters JSON Schema as `serde_json::Value`).
    - Add `ToolChoice` enum: `auto | none` (milestone-1 only).
    - Extend `CompletionRequest` with `tools: Option<Vec<ToolDef>>` and `tool_choice: Option<ToolChoice>`.
    - Extend `ProviderStreamEvent` with:
      - `ToolCallDelta { tool_call_id, function_name: Option<String>, arguments_delta }`
      - `ToolCallComplete { tool_call_id, function_name, arguments_json }`
  - Update all workspace call sites to compile (mock provider, openai provider, harness-core agent runtime).
  - Add/adjust unit tests in `crates/harness-providers` to ensure serde roundtrip and event ordering stability.

  **Must NOT do**:
  - Do NOT encode tool calls as `TextDelta("[tool_call:...]"...)`.
  - Do NOT introduce provider-specific types into `harness-core` (keep provider contract in `harness-providers`).

  **Recommended Agent Profile**:
  - Category: `unspecified-high` — multi-crate API change with many consumers.
  - Skills: [`rust-best-practices`] — keep Rust types/derives idiomatic.
  - Omitted: [`git-master`] — commit strategy handled per-task, not needed to edit files.

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 2,3,4,9 | Blocked By: -

  **References**:
  - Provider contract baseline: `crates/harness-providers/src/lib.rs:10-58`
  - Mock provider tool-call currently fake-encoded: `crates/harness-providers/src/mock.rs:171-213`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-providers -- --test-threads=1`
  - [ ] Workspace compiles for provider crate consumers (checked by later full-workspace commands).

  **QA Scenarios**:
  ```
  Scenario: Provider types compile + tests pass
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Run: cargo test -p harness-providers -- --test-threads=1
    Expected:
      - All tests pass; no clippy/compile errors introduced by new types.
    Evidence: .sisyphus/evidence/task-1-provider-contract.txt

  Scenario: Serde stability for new request fields
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Ensure at least one unit test covers JSON serialization including tools/tool_choice
    Expected:
      - Deterministic JSON output; no panic in serde.
    Evidence: .sisyphus/evidence/task-1-serde.txt
  ```

  **Commit**: YES | Message: `feat(providers): add structured tool-calling contract` | Files: [`crates/harness-providers/src/lib.rs`]

- [x] 2. OpenAI Chat Completions: send tools + parse streamed tool calls into ProviderStreamEvent

  **What to do**:
  - Update `crates/harness-providers/src/openai.rs`:
    - Extend `OpenAiChatCompletionsRequest` serialization to include `tools` + `tool_choice` derived from `CompletionRequest`.
    - Extend the chat SSE chunk types to parse `choices[].delta.tool_calls[]` (call id, function name, arguments deltas).
    - Update `consume_chat_sse_stream()` to emit `ProviderStreamEvent::ToolCallDelta` as tool-call arguments stream.
    - Aggregate per `tool_call_id` and emit `ProviderStreamEvent::ToolCallComplete` once finish_reason indicates tool calls (or stream ends), failing closed if JSON cannot parse.
  - Add offline wiremock tests (similar style to existing tests) proving:
    - tool-call args reconstruction across multiple deltas
    - malformed JSON fails closed with `ProviderStreamEvent::Error`

  **Must NOT do**:
  - Do NOT depend on `inspiration/` code; only use it to understand event shapes.
  - Do NOT drop existing text streaming behavior.

  **Recommended Agent Profile**:
  - Category: `unspecified-high` — streaming protocol parsing + tests.
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 9 | Blocked By: 1

  **References**:
  - Existing chat SSE parsing loop: `crates/harness-providers/src/openai.rs:214-304`
  - Existing chunk structs (content-only): `crates/harness-providers/src/openai.rs:553-573`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-providers openai_compatible -- --test-threads=1`
  - [ ] New test(s) assert tool-call events emitted for chat-completions SSE fixture(s).

  **QA Scenarios**:
  ```
  Scenario: Chat-completions tool-call stream reconstructs JSON
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add a wiremock SSE transcript that streams tool_calls.arguments in multiple chunks
      2) Run: cargo test -p harness-providers openai_compatible -- --test-threads=1
    Expected:
      - Stream includes ToolCallDelta events, then ToolCallComplete with parsed JSON.
    Evidence: .sisyphus/evidence/task-2-chat-tool-calls.txt

  Scenario: Malformed JSON args fails closed
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add a transcript with invalid JSON in tool_calls.arguments
      2) Run same test
    Expected:
      - Provider emits Error event and terminates; no ToolCallComplete.
    Evidence: .sisyphus/evidence/task-2-chat-malformed.txt
  ```

  **Commit**: YES | Message: `feat(providers): chat-completions tool calling` | Files: [`crates/harness-providers/src/openai.rs`]

- [x] 3. OpenAI Responses: send tools + parse streamed function calls into ProviderStreamEvent

  **What to do**:
  - Update `crates/harness-providers/src/openai.rs`:
    - Extend `OpenAiResponsesRequest` serialization to include `tools` + `tool_choice` (milestone-1: only auto/none).
    - Extend `consume_responses_sse_stream()` to recognize Responses tool-call related event types and normalize into ToolCallDelta/Complete.
    - Implement a per-call accumulator keyed by call/item id.
    - Fail closed if completion event arrives but args JSON cannot parse.
  - Add offline wiremock tests proving:
    - Responses tool-call events produce ToolCallComplete
    - malformed args JSON fails closed

  **Must NOT do**:
  - Do NOT regress existing `response.output_text.delta` behavior.
  - Do NOT assume a single tool call per response; support multiple but execute sequentially later.

  **Recommended Agent Profile**:
  - Category: `unspecified-high`
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 9 | Blocked By: 1

  **References**:
  - Existing Responses SSE parsing (text-only): `crates/harness-providers/src/openai.rs:306-409`
  - Inspiration event-shape reference (read-only): `inspiration/pi_agent_rust/src/providers/openai_responses.rs:102-165`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-providers openai_responses_offline -- --test-threads=1`
  - [ ] New test(s) assert ToolCallComplete emitted for Responses SSE fixture(s).

  **QA Scenarios**:
  ```
  Scenario: Responses tool-call stream reconstructs JSON
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add a wiremock SSE transcript with tool-call events
      2) Run: cargo test -p harness-providers openai_responses_offline -- --test-threads=1
    Expected:
      - Stream includes ToolCallDelta and ToolCallComplete events.
    Evidence: .sisyphus/evidence/task-3-responses-tool-calls.txt

  Scenario: Responses malformed JSON args fails closed
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add malformed args transcript
      2) Re-run test
    Expected:
      - Provider emits Error and stops.
    Evidence: .sisyphus/evidence/task-3-responses-malformed.txt
  ```

  **Commit**: YES | Message: `feat(providers): responses tool calling` | Files: [`crates/harness-providers/src/openai.rs`]

- [x] 4. MockProvider: emit structured tool-call events from fixtures (no fake TextDelta encoding)

  **What to do**:
  - Update `crates/harness-providers/src/mock.rs` fixture mapping:
    - Convert `FixtureStreamEvent::ToolCall` into `ProviderStreamEvent::ToolCallComplete` (and optionally ToolCallDelta for realism).
    - Stop encoding tool calls into `TextDelta`.
  - Update `FixtureCompletionRequest` → `CompletionRequest` mapping to include `tools` and `tool_choice` fields as needed by new contract.
  - Ensure existing fixture `crates/harness-testkit/fixtures/mock_provider/tool_call_stream.json` still loads and produces structured tool call events.
  - Add/adjust unit tests in mock provider module verifying tool-call fixture produces ToolCallComplete.

  **Must NOT do**:
  - Do NOT change request digest algorithm semantics beyond including newly-added fields deterministically.

  **Recommended Agent Profile**:
  - Category: `quick` — localized fixture + mapping changes.
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 9,18+ | Blocked By: 1

  **References**:
  - Fixture tool-call file: `crates/harness-testkit/fixtures/mock_provider/tool_call_stream.json:1-47`
  - Current fake encoding: `crates/harness-providers/src/mock.rs:205-213`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-providers mock -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: Tool-call fixture produces ToolCallComplete event
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Run: cargo test -p harness-providers mock -- --test-threads=1
    Expected:
      - Test asserts ToolCallComplete present (no TextDelta tool-call encoding).
    Evidence: .sisyphus/evidence/task-4-mock-fixtures.txt
  ```

  **Commit**: YES | Message: `test(providers): mock fixtures emit structured tool calls` | Files: [`crates/harness-providers/src/mock.rs`, `crates/harness-testkit/fixtures/mock_provider/tool_call_stream.json`]

- [x] 5. Tool specifications: add tool metadata (description + JSON Schema) and deterministic tool-id → function-name mapping

  **What to do**:
  - Extend `harness-core` tool trait so tools can be advertised to providers:
    - Update `crates/harness-core/src/tool.rs` `Tool` trait (currently only id/capability/call) to also expose:
      - `fn description(&self) -> &str`
      - `fn parameters_json_schema(&self) -> serde_json::Value`
  - Implement these methods for existing tools in `crates/harness-tools/src/lib.rs` and `crates/harness-tools/src/hashline_apply.rs`.
    - Add `schemars` dependency to `harness-tools` and derive `JsonSchema` for args structs where convenient.
  - Add a deterministic function-name sanitizer in `harness-core` (or `harness-providers` if preferred) that converts canonical tool ids (`fs.read`) into provider-safe function names (`fs_read`).
    - Must be reversible via a map for the current request (function_name → tool_id).
    - Milestone-1 policy: replace any non `[A-Za-z0-9_-]` with `_`; if leading char is not alphabetic/underscore, prefix `t_`.
  - Add unit tests verifying:
    - schemas are valid JSON objects
    - function-name mapping is deterministic and unique for current registry

  **Must NOT do**:
  - Do NOT introduce tool aliases in M1 (toolset uses canonical ids only).
  - Do NOT add new tools in this task (only specs for existing ones).

  **Recommended Agent Profile**:
  - Category: `unspecified-high` — touches core tool trait + tools crate.
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 9,11-16 | Blocked By: -

  **References**:
  - Tool trait baseline: `crates/harness-core/src/tool.rs:223-230`
  - Existing built-in tools + args structs: `crates/harness-tools/src/lib.rs:34-183`
  - Hashline apply tool: `crates/harness-tools/src/hashline_apply.rs:10-71`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core -- --test-threads=1`
  - [ ] `cargo test -p harness-tools -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: Tool specs available for provider advertisement
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Run: cargo test -p harness-tools -- --test-threads=1
    Expected:
      - Tool implementations compile with new trait methods.
    Evidence: .sisyphus/evidence/task-5-tool-specs.txt

  Scenario: Function-name mapping deterministic
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add unit test asserting tool_id → function_name stable and unique
      2) Run: cargo test -p harness-core -- --test-threads=1
    Expected:
      - Test passes; mapping stable.
    Evidence: .sisyphus/evidence/task-5-function-name-mapping.txt
  ```

  **Commit**: YES | Message: `feat(tools): expose tool descriptions and parameter schemas` | Files: [`crates/harness-core/src/tool.rs`, `crates/harness-tools/src/lib.rs`, `crates/harness-tools/src/hashline_apply.rs`, `crates/harness-tools/Cargo.toml`]

- [x] 6. Coordinator auth derivation: derive category + toolset from `actor.agent_id`; ignore caller category; add UserMessageSubmitted events

  **What to do**:
  - In `crates/harness-core/src/coord.rs`:
    - Update `request_tool_call_internal()` to compute `effective_category` from `actor.agent_id → run_state.agents[agent_id].category`.
      - Ignore the caller-supplied `category` for permission policy evaluation.
      - If `actor.kind == Worker` and `actor.agent_id` is missing/unknown → fail closed with `PolicyViolationDetected` + error.
    - Enforce toolset allowlist for workers:
      - If `actor.kind == Worker`, require `tool_id` ∈ `run_state.agents[agent_id].toolset`.
      - Deny with `PolicyViolationDetected(policy="tool_not_in_toolset")` and return error.
    - Add `UserMessageSubmitted` event emission in `request_agent_turn_internal()` once `request_id` is allocated.
      - Use correlation_id=request_id so TUI groups user prompt with provider deltas.
  - Update scenario profiles so the scenario runner’s worker tool call still succeeds under toolset enforcement:
    - `crates/harness/src/scenarios.rs:170-193` add `edit.hashline_apply` to worker toolset.
  - Add unit tests in `crates/harness-core/tests/` validating:
    - caller category is ignored (derived category used)
    - unknown worker agent id fails closed
    - toolset enforcement works

  **Must NOT do**:
  - Do NOT remove the `category` argument from the public `CoordinatorHandle.request_tool_call` API in M1 (keep compatibility), but it must not influence auth.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — authorization logic + safety invariants.
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 7,8,9,16,17,18+ | Blocked By: -

  **References**:
  - request_tool_call_internal + permission decision uses category today: `crates/harness-core/src/coord.rs:953-1032`
  - request_agent_turn_internal allocates request_id: `crates/harness-core/src/coord.rs:924-950`
  - Agent profiles include category/toolset: `crates/harness-core/src/agent.rs:13-19`
  - Scenario worker tool call site: `crates/harness/src/tui.rs:461-468`
  - Scenario profiles baseline: `crates/harness/src/scenarios.rs:170-193`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core -- --test-threads=1`
  - [ ] Scenario runner still succeeds in CI PTY runs (validated later).

  **QA Scenarios**:
  ```
  Scenario: Tool category spoof is ignored
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add a harness-core test that calls request_tool_call with category="deep" but actor’s profile.category is different
    Expected:
      - Permission policy evaluation uses derived category, not caller param.
    Evidence: .sisyphus/evidence/task-6-category-derivation.txt

  Scenario: Toolset enforcement denies unknown tool
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add test: worker actor requests tool not in toolset
    Expected:
      - PolicyViolationDetected emitted; coordinator returns PolicyViolation error.
    Evidence: .sisyphus/evidence/task-6-toolset-enforcement.txt
  ```

  **Commit**: YES | Message: `feat(core): derive tool auth from actor agent_id` | Files: [`crates/harness-core/src/coord.rs`, `crates/harness/src/scenarios.rs`, `crates/harness-core/tests/*`]

- [x] 7. Enforce Scheduler limits for tool execution (queue tool tasks instead of starting immediately)

  **What to do**:
  - Refactor tool execution scheduling in `crates/harness-core/src/coord.rs`:
    - Tool tasks must call `run_state.scheduler.schedule(task_id, ConcurrencyKey::Tool{tool_id})` (same as provider turns) instead of starting immediately.
    - If queued:
      - Append `TaskScheduled(state=Queued, queue_key=tool:<tool_id>)`.
      - Store a `QueuedToolCall` record in `run_state` (new map) keyed by `task_id`.
      - Do NOT emit `ToolCallStarted` yet.
    - If started:
      - Emit `ToolCallStarted` and then spawn the tool call task as today.
  - On task completion (`job_finished_internal`) dequeue via `scheduler.complete(&key)` and start any queued tool calls (mirror `queued_agent_turns` flow in `agent_turn_finished_internal`).
  - Update cancellation path `cancel_task_internal` to cancel queued tool calls via `scheduler.cancel_queued()`.
  - Add unit/integration tests proving `tool_concurrency=1` causes queuing.

  **Must NOT do**:
  - Do NOT change scheduler semantics for provider turns.
  - Do NOT emit `ToolCallStarted` for queued tool calls.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — concurrency + state machine correctness.
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: NO | Wave 3 | Blocks: 8,9,10,16 | Blocked By: 6

  **References**:
  - Scheduler API: `crates/harness-core/src/sched.rs:108-147`
  - Tool execution currently starts immediately: `crates/harness-core/src/coord.rs:1727-1889`
  - Provider turn queuing example: `crates/harness-core/src/coord.rs:1915-1975`
  - Dequeue + start queued agent turns: `crates/harness-core/src/coord.rs:1612-1655`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: Tool concurrency queues second tool call
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add a test with tool_concurrency=1
      2) Request two tool calls rapidly
    Expected:
      - First tool starts; second emits TaskScheduled(Queued) and starts only after first completes.
    Evidence: .sisyphus/evidence/task-7-tool-scheduler.txt
  ```

  **Commit**: YES | Message: `feat(core): apply scheduler gating to tool tasks` | Files: [`crates/harness-core/src/coord.rs`]

- [x] 8. Awaitable tool execution: add `CallToolAndWait` command + tool-call waiter map (covers permission ask/deny/timeout)

  **What to do**:
  - Extend `crates/harness-core/src/coord.rs` command API:
    - Add `Command::CallToolAndWait { actor, tool_id, args_json, respond_to }` returning a `ToolCallWaitOutcome` (tool_call_id + status + optional ToolResult or error).
    - Add `CoordinatorHandle::call_tool_and_wait(...)` wrapper method.
  - Implement a `run_state.tool_waiters` map keyed by `tool_call_id` → oneshot sender.
    - Register waiter when CallToolAndWait starts.
    - Resolve waiter on all completion paths:
      - tool task succeeded/failed/cancelled in `job_finished_internal`
      - permission denied in `finalize_permission_denied`
      - permission timeout in `resolve_permission_timeout_internal`
      - explicit deny in `resolve_permission_internal` deny branch
  - Add tests proving:
    - Ask permission path unblocks when permission resolved
    - timeout denies and returns failed outcome within bound
    - cancellation does not deadlock (late results treated as late)

  **Must NOT do**:
  - Do NOT implement awaiting by polling JSONL on disk.
  - Do NOT block the coordinator command loop awaiting tool completion.

  **Recommended Agent Profile**:
  - Category: `ultrabrain`
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: NO | Wave 3 | Blocks: 9,16 | Blocked By: 6,7

  **References**:
  - Command enum baseline: `crates/harness-core/src/coord.rs:124-211`
  - Permission ask path stores pending permissions: `crates/harness-core/src/coord.rs:1047-1079`
  - Tool completion currently only appends events: `crates/harness-core/src/coord.rs:1337-1491`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: call_tool_and_wait succeeds
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add a test tool that returns ToolResult::text("ok")
      2) Call CoordinatorHandle.call_tool_and_wait as worker actor
    Expected:
      - Returns succeeded outcome with ToolResult.
    Evidence: .sisyphus/evidence/task-8-tool-await-ok.txt

  Scenario: permission Ask blocks then resolves
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Configure permission policy Ask for edit_fs
      2) call_tool_and_wait(edit.hashline_apply)
      3) Resolve permission Allow via coordinator
    Expected:
      - Await returns after tool executes; no deadlock.
    Evidence: .sisyphus/evidence/task-8-permission-await.txt
  ```

  **Commit**: YES | Message: `feat(core): awaitable tool execution API` | Files: [`crates/harness-core/src/coord.rs`]

- [x] 9. Multi-turn tool-aware agent loop: execute provider→tools→provider until completion (bounded + fail-closed)

  **What to do**:
  - Implement a multi-turn runner in `crates/harness-core/src/agent.rs` that:
    - Builds message history: System(profile.system_prompt) + User(initial prompt) + (Assistant/Tool messages as loop progresses).
    - Includes tool definitions derived from agent profile toolset + tool registry (Task 5).
    - Streams provider events and emits existing `AgentRuntimeEvent::{ProviderRequestStarted,ProviderStreamDelta,ProviderRequestFinished}`.
    - Collects `ProviderStreamEvent::ToolCallComplete` events from the provider stream; for each tool call:
      - Map `function_name → tool_id` using the current request’s tool defs (fail closed if unmapped).
      - Call coordinator via `CallToolAndWait` (Task 8) as the worker actor for `agent_id`.
      - Append a Tool-role message with the same `tool_call_id` and `name=function_name`.
    - Guardrails (milestone-1 constants, configurable later):
      - `MAX_ITERS = 12`
      - `MAX_TOOL_CALLS_TOTAL = 25`
      - Fail closed on malformed JSON args, unknown tool function names, permission denial, or tool failure.
    - Stop condition: provider finishes a turn with **zero** tool calls → return final output.
  - Integrate into coordinator execution:
    - Replace `run_single_turn_streaming(...)` call site in `crates/harness-core/src/coord.rs:start_agent_turn_execution` with the new multi-turn runner.
    - Use a tool-caller adapter that sends `Command::CallToolAndWait` over `job_tx`.
  - Add harness-core tests that:
    - run multi-turn against `MockProvider` fixture emitting a tool call then a final completion
    - cover failure modes: unmapped function name; tool args invalid JSON; tool returns failure; permission denied/timeout

  **Must NOT do**:
  - Do NOT allow infinite loops; guardrails must abort with deterministic error reasons.
  - Do NOT silently ignore tool calls.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — concurrent state machine + correctness.
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: NO | Wave 4 | Blocks: 16-20 | Blocked By: 1,2,3,5,6,7,8

  **References**:
  - Single-turn runner today: `crates/harness-core/src/agent.rs:101-192`
  - Coordinator runs single-turn in background task: `crates/harness-core/src/coord.rs:1980-2074`
  - Tool calling capability in provider layer (after Tasks 1-3): `crates/harness-providers/src/lib.rs`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: Tool call → tool executes → model completes
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add mock-provider fixture: first turn emits ToolCallComplete(fs_read), second turn emits final text
      2) Add harness-core test wiring CallToolAndWait to real tool registry
      3) Run: cargo test -p harness-core -- --test-threads=1
    Expected:
      - Test asserts tool called exactly once, tool result reinjected, loop terminates within MAX_ITERS.
    Evidence: .sisyphus/evidence/task-9-multi-turn-happy.txt

  Scenario: Unmapped function name fails closed
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Mock tool call uses function_name not in advertised tool defs
    Expected:
      - Turn fails with deterministic error; no tool execution.
    Evidence: .sisyphus/evidence/task-9-unmapped-tool.txt
  ```

  **Commit**: YES | Message: `feat(core): multi-turn tool-aware agent loop` | Files: [`crates/harness-core/src/agent.rs`, `crates/harness-core/src/coord.rs`, `crates/harness-core/tests/*`]

- [x] 10. Tool output persistence policy: cap JSONL summaries; store bulky redacted outputs as artifacts; extend secret scan to artifacts

  **What to do**:
  - In `crates/harness-core/src/coord.rs` tool completion path (`job_finished_internal`):
    - Stop storing unbounded `ToolResult.display_text` into `TaskCompleted.result_summary` and `ToolCallFinished.output_summary`.
    - Define summary cap constants (e.g., 2_000 chars for summary; store digest of full redacted output separately in artifact metadata).
    - Always create a redacted JSON artifact for tool outputs:
      - Suggested path: `toolcalls/{tool_call_id}/result.redacted.json`
      - Contents includes: tool_id, status, display_text, structured_json, artifacts[]
    - If display_text is large, also create `toolcalls/{tool_call_id}/display.redacted.txt`.
    - Append `ArtifactWritten` events for these coordinator-generated artifacts.
  - Update redaction/secret-scan helper in `crates/harness-core/src/redact.rs` to also scan session `artifacts/**` files for `sk-` leakage (at minimum `.json`, `.jsonl`, `.txt`, snapshot files).
  - Add tests verifying:
    - artifacts are written
    - summaries are capped
    - secret scan fails on unredacted artifacts and passes on redacted

  **Must NOT do**:
  - Do NOT write raw (unredacted) provider credentials into artifacts.
  - Do NOT change event schema version.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — correctness + safety + persistence.
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: NO | Wave 3 | Blocks: 17,19 | Blocked By: 6,7

  **References**:
  - Current event bloat source: `crates/harness-core/src/coord.rs:1397-1428`
  - Artifact store helper: `crates/harness-core/src/tool.rs:102-152`
  - Redaction + secret scan helper: `crates/harness-core/src/redact.rs:52-166`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core redact -- --test-threads=1`
  - [ ] `cargo test -p harness-core -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: Large tool output is capped in events and spilled to artifact
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add test tool returning very large display_text containing a fake api key pattern
      2) Execute tool call and inspect emitted events + artifact contents
    Expected:
      - Event summaries do not contain raw secret; artifacts are redacted; secret scan passes.
    Evidence: .sisyphus/evidence/task-10-artifact-redaction.txt
  ```

  **Commit**: YES | Message: `feat(core): cap tool summaries and persist redacted tool artifacts` | Files: [`crates/harness-core/src/coord.rs`, `crates/harness-core/src/redact.rs`]

- [x] 11. Add `edit.hashline_scan` tool (line hashes/anchors) to support authoring HashlinePatch without manual hashing

  **What to do**:
  - Implement new tool in `crates/harness-tools`:
    - Tool id: `edit.hashline_scan`
    - Capability: `ToolCapability::ReadFs`
    - Args (milestone-1): `{ path: string, start_line?: u32, limit?: u32 }`
      - path must be workspace-relative
      - defaults: start_line=1, limit=2000
    - Output:
      - `structured_json`: `{ path, resolved_path, start_line, limit, anchors: [{ line, hash, text }] }`
      - `display_text`: line-numbered list like `12 abcdef123456 | let x = ...`
      - Also write `artifacts/hashline_scan/{sanitized_path}.json` with the structured output (redacted; should be safe).
  - Add tool schema/description (Task 5 contract).
  - Add unit tests for:
    - anchor hashes match `compute_line_hash`
    - out-of-range start_line handled gracefully

  **Must NOT do**:
  - Do NOT allow absolute paths or workspace escapes.

  **Recommended Agent Profile**:
  - Category: `unspecified-high`
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 19 | Blocked By: 5

  **References**:
  - Hashline hashing primitive: `crates/harness-core/src/edit/hashline.rs:198-205`
  - Existing edit tool pattern: `crates/harness-tools/src/hashline_apply.rs:22-71`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tools -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: hashline_scan returns anchors for demo file
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add unit test using temp workspace file with known lines
    Expected:
      - Returned hashes equal compute_line_hash(line_text)
    Evidence: .sisyphus/evidence/task-11-hashline-scan.txt
  ```

  **Commit**: YES | Message: `feat(tools): add edit.hashline_scan` | Files: [`crates/harness-tools/src/lib.rs`, `crates/harness-tools/src/hashline_scan.rs` (new), `crates/harness-tools/Cargo.toml`]

- [x] 12. Upgrade `fs.read` tool for coding workflows (offset/limit + line numbers + truncation)

  **What to do**:
  - Update `FsReadTool` in `crates/harness-tools/src/lib.rs`:
    - Args (milestone-1): `{ path: string, offset?: u32, limit?: u32, line_numbers?: bool }`
      - offset is 1-indexed line number (default 1)
      - limit default 2000
      - line_numbers default true
    - Behavior:
      - Reject absolute paths and workspace escapes (keep existing checks).
      - Read file as UTF-8; if invalid UTF-8 → return ToolError::Execution("binary file not supported") (M1).
      - Return formatted, line-numbered output with truncation marker if needed.
      - Provide `structured_json` including `{path,resolved_path,offset,limit,total_lines,truncated}`.
      - Spill full (redacted) output to artifact when truncated.
  - Update tool schema/description (Task 5 contract).
  - Add unit tests for:
    - offset/limit
    - truncation marker
    - binary rejection

  **Must NOT do**:
  - Do NOT return unlimited file contents in events.

  **Recommended Agent Profile**:
  - Category: `unspecified-high`
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 19 | Blocked By: 5

  **References**:
  - Current fs.read implementation (no limits): `crates/harness-tools/src/lib.rs:34-80`
  - Artifact store: `crates/harness-core/src/tool.rs:97-152`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tools -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: Read README with line numbers and limit
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add unit test reading a multi-line fixture file
      2) Assert output contains "1:" prefix and truncation marker when limit small
    Expected:
      - Output is deterministic; structured_json.truncated matches.
    Evidence: .sisyphus/evidence/task-12-fs-read.txt
  ```

  **Commit**: YES | Message: `feat(tools): improve fs.read (offset/limit/line numbers)` | Files: [`crates/harness-tools/src/lib.rs`]

- [x] 13. Add `fs.ls` tool (directory listing) with safe limits

  **What to do**:
  - Implement `fs.ls` in `crates/harness-tools`:
    - Capability: `ToolCapability::ReadFs`
    - Args: `{ path: string, limit?: u32 }` (default limit=2000)
    - Behavior: list immediate children of directory; append `/` for directories; deterministic sort.
    - Output: `display_text` = newline separated entries; `structured_json` includes entries and counts.
  - Add tool schema/description (Task 5).
  - Add unit tests for:
    - deterministic sorting
    - limit truncation marker

  **Must NOT do**:
  - Do NOT recurse.

  **Recommended Agent Profile**:
  - Category: `quick`
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 19 | Blocked By: 5

  **References**:
  - Tool registry wiring: `crates/harness-tools/src/lib.rs:15-32`
  - Workspace path resolution: `crates/harness-core/src/tool.rs:69-95`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tools -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: List workspace root
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add unit test using temp dir with files + subdir
    Expected:
      - Output sorted and directories have trailing '/'.
    Evidence: .sisyphus/evidence/task-13-fs-ls.txt
  ```

  **Commit**: YES | Message: `feat(tools): add fs.ls` | Files: [`crates/harness-tools/src/lib.rs`, `crates/harness-tools/src/fs_ls.rs` (new)]

- [x] 14. Add `fs.glob` tool (file discovery) with deterministic ordering and caps

  **What to do**:
  - Implement `fs.glob` in `crates/harness-tools`:
    - Capability: `ToolCapability::ReadFs`
    - Args: `{ pattern: string, path?: string, limit?: u32 }`
      - `path` is base dir relative to workspace root (default ".")
      - `pattern` supports `**` style globs (document exact support; pick a crate and commit to semantics).
      - limit default 100.
    - Behavior: walk base dir, match pattern, return relative paths; deterministic sort.
  - Add tool schema/description.
  - Add unit tests with a temp workspace verifying `**/*.rs` works.

  **Must NOT do**:
  - Do NOT scan `target/` by default; explicitly skip common build dirs (documented list).

  **Recommended Agent Profile**:
  - Category: `unspecified-high`
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 19 | Blocked By: 5

  **References**:
  - Existing tool patterns: `crates/harness-tools/src/lib.rs:41-79`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tools -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: Glob finds files deterministically
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Create temp dir tree with known files
      2) Call tool with pattern "**/*.txt"
    Expected:
      - Returned paths match expected and are sorted.
    Evidence: .sisyphus/evidence/task-14-fs-glob.txt
  ```

  **Commit**: YES | Message: `feat(tools): add fs.glob` | Files: [`crates/harness-tools/src/lib.rs`, `crates/harness-tools/src/fs_glob.rs` (new), `crates/harness-tools/Cargo.toml`]

- [x] 15. Add `fs.grep` tool (content search) with regex + caps + binary skipping

  **What to do**:
  - Implement `fs.grep` in `crates/harness-tools`:
    - Capability: `ToolCapability::ReadFs`
    - Args: `{ pattern: string, path?: string, include?: string, limit?: u32, context?: u32 }`
      - `pattern` is Rust regex
      - `path` base dir (default ".")
      - `include` optional glob filter for file names
      - limit default 100 matches
      - context default 0
    - Behavior:
      - Walk files under base dir (skip `target/`, `.git/`, session dir)
      - Skip files that are not valid UTF-8
      - Emit matches as `path:line: text` with optional context lines
      - Deterministic order: path, then line number
  - Add schema/description.
  - Add unit tests on temp workspace.

  **Must NOT do**:
  - Do NOT shell out to `rg` in M1.

  **Recommended Agent Profile**:
  - Category: `unspecified-high`
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 19 | Blocked By: 5

  **References**:
  - Regex already used in core: `crates/harness-core/src/redact.rs:1-29`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tools -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: Grep finds TODOs with context
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Create temp files containing "TODO" on known lines
      2) Call fs.grep(pattern="TODO", context=1)
    Expected:
      - Output includes correct lines and context; match count <= limit.
    Evidence: .sisyphus/evidence/task-15-fs-grep.txt
  ```

  **Commit**: YES | Message: `feat(tools): add fs.grep` | Files: [`crates/harness-tools/src/lib.rs`, `crates/harness-tools/src/fs_grep.rs` (new), `crates/harness-tools/Cargo.toml`]

- [x] 16. Multi-agent delegation (MVP): add `agent.delegate` tool + awaitable agent-turn completion API + guardrails

  **What to do**:
  - Add coordinator support to await agent-turn completion (needed for delegation):
    - Add `Command::RequestAgentTurnAndWait { actor, agent_id, prompt, respond_to }`.
      - Coordinator schedules the agent turn (existing path) and registers a waiter keyed by request_id.
      - Resolve waiter in `agent_turn_finished_internal` using the request_id passed to it.
    - Add `CoordinatorHandle::request_agent_turn_and_wait(...)` method.
  - Add `agent.delegate` tool in `crates/harness-tools`:
    - Capability: `ToolCapability::SpawnAgent`
    - Args: `{ profile: string, prompt: string }`
    - Implementation:
      - Use a coordinator handle carried in `ToolContext` (add `coordinator: CoordinatorHandle` to `ToolContext` in `crates/harness-core/src/tool.rs`).
      - Spawn a child agent idle with `parent_agent_id = ctx.actor.agent_id`.
      - Run exactly one agent turn using `request_agent_turn_and_wait`.
      - Return ToolResult where display_text is the child output, and structured_json includes `{ child_agent_id, child_request_id, profile }`.
    - Add hard guardrails in coordinator for this tool id:
      - Max delegations per run: 4 (fail closed with policy violation when exceeded).
  - Enable workers to call SpawnAgent-capability tools (delegation) without allowing direct spawning APIs:
    - Add `ToolCapability::SpawnAgent` to `actor_capabilities(ActorKind::Worker)`.
    - Keep `spawn_agent_internal` restriction (Supervisor-only) unchanged.
  - Tests:
    - harness-core tests for RequestAgentTurnAndWait
    - harness-tools tests for agent.delegate using InMemoryEventStore + MockProvider

  **Must NOT do**:
  - Do NOT implement streaming delegation in M1; return one-shot child output.
  - Do NOT relax `CoordinatorHandle.spawn_agent` supervisor-only restriction.

  **Recommended Agent Profile**:
  - Category: `ultrabrain`
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: NO | Wave 4 | Blocks: 19 | Blocked By: 6,8,9

  **References**:
  - Spawn agent restriction: `crates/harness-core/src/coord.rs:808-839`
  - Request agent turn restriction + request_id allocation: `crates/harness-core/src/coord.rs:896-951`
  - Worker lacks SpawnAgent capability today: `crates/harness-core/src/tool.rs:281-298`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core -- --test-threads=1`
  - [ ] `cargo test -p harness-tools -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: Delegate spawns child and returns output
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Use MockProvider scripted to return deterministic output for child prompt
      2) Call agent.delegate and assert ToolResult.display_text equals expected
    Expected:
      - Child agent spawned with parent_agent_id set; request awaited; no deadlocks.
    Evidence: .sisyphus/evidence/task-16-delegate.txt

  Scenario: Delegation guardrail blocks 5th delegation
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Call agent.delegate 5 times
    Expected:
      - 5th call fails with policy violation and emits PolicyViolationDetected.
    Evidence: .sisyphus/evidence/task-16-delegate-guardrail.txt
  ```

  **Commit**: YES | Message: `feat(core/tools): add delegation (agent.delegate) MVP` | Files: [`crates/harness-core/src/coord.rs`, `crates/harness-core/src/tool.rs`, `crates/harness-tools/src/lib.rs`, `crates/harness-tools/src/agent_delegate.rs` (new)]

- [x] 17. TUI visibility: show tool calls/results + agent spawns in Activity/Transcript; fix header profile source

  **What to do**:
  - Update `crates/harness-tui/src/app.rs` derived state ingestion to handle:
    - `EventV1::ToolCallRequested/Started/Finished`
    - `EventV1::AgentSpawned`
  - Refactor Activity model:
    - Replace/extend `ActivityEntry` so the Activity pane can list BOTH provider request activities and tool call activities.
      - Recommended: `enum ActivityItem { Provider { request_id, ... }, ToolCall { tool_call_id, tool_id, status, args_summary, output_summary, artifacts... } }`.
  - Update `crates/harness-tui/src/ui.rs`:
    - Header: show active profile derived from latest `AgentSpawned.profile` (remove hardcoded `"default"`).
    - Activity pane: render tool call entries with status.
    - Transcript pane: when a tool call entry selected, show args_summary + output_summary + artifact list.
  - Add/adjust unit tests in `crates/harness-tui/src/lib.rs` verifying:
    - ingesting tool call events creates Activity item
    - selecting tool call renders expected text in buffer

  **Must NOT do**:
  - Do NOT change the PTY-tested keybindings unless you also update PTY tests.

  **Recommended Agent Profile**:
  - Category: `visual-engineering` — terminal UI state + rendering.
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 19 | Blocked By: 9,10,11,12,13,14,15,16

  **References**:
  - TUI derived state today (provider-only): `crates/harness-tui/src/app.rs:761-859`
  - Header profile TODO: `crates/harness-tui/src/ui.rs:46-70`
  - Tool call events schema: `crates/harness-core/src/event.rs:95-235`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tui -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: Tool call visible in TUI state
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Add unit test: ingest ToolCallRequested+Finished events
      2) Render frame and assert buffer contains tool_id and status
    Expected:
      - Buffer includes tool call rows; selection renders args/output.
    Evidence: .sisyphus/evidence/task-17-tui-tool-visibility.txt
  ```

  **Commit**: YES | Message: `feat(tui): render tool calls and agent spawns` | Files: [`crates/harness-tui/src/app.rs`, `crates/harness-tui/src/ui.rs`, `crates/harness-tui/src/lib.rs`]

- [x] 18. Harness TUI interactive mode: use config-based provider/profiles on `--config` (real interactive coding path)

  **What to do**:
  - Update `crates/harness/src/tui.rs` interactive mode (`run_interactive_mode`) behavior:
    - If a config file is resolved (explicit `--config` or default `harness.jsonc`):
      - Load config via `bootstrap::load_harness_config`
      - Build coordinator config via `bootstrap::build_interactive_coordinator_config`
      - Workspace root: current working directory (real project), NOT `create_workspace(...)` sandbox.
      - Spawn agent using `settings.default_profile` (must match a category key).
      - Apply UI keybindings from config (`cfg.ui.keybindings`) into TUI options.
    - If no config is present:
      - Keep current sandboxed mock-provider flow (golden path) for demo-only.
  - Update error messages so missing/invalid config is actionable.
  - Update `crates/harness-testkit/tests/pty_e2e.rs` helper `write_wiremock_tui_config` to produce a config compatible with config-based TUI mode:
    - `ui.default_profile` must correspond to an actual category (e.g. `deep` or `worker`), and category tools must use canonical tool ids (`fs.read`, not `read`).

  **Must NOT do**:
  - Do NOT change scenario mode behavior (`--scenario ...`) — PTY tests rely on it.

  **Recommended Agent Profile**:
  - Category: `unspecified-high` — cross-cutting CLI bootstrapping + test updates.
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: NO | Wave 4 | Blocks: 19 | Blocked By: 17

  **References**:
  - Current interactive mode uses golden_path_provider/profiles: `crates/harness/src/tui.rs:190-305`
  - Config-based coordinator builder: `crates/harness/src/bootstrap.rs:24-100`
  - Prompt mode already uses config-based coordinator: `crates/harness/src/prompt.rs:95-199`
  - PTY config generator currently inconsistent: `crates/harness-testkit/tests/pty_e2e.rs:320-347`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-testkit pty_e2e_tui_interactive_prompt_streams_response -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: TUI uses config provider when --config is passed
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Run PTY test that launches `harness tui --config <wiremock-config>`
    Expected:
      - Wiremock server receives requests; UI shows streamed output.
    Evidence: .sisyphus/evidence/task-18-tui-config-mode.txt
  ```

  **Commit**: YES | Message: `feat(harness): config-based interactive TUI mode` | Files: [`crates/harness/src/tui.rs`, `crates/harness-testkit/tests/pty_e2e.rs`]

- [x] 19. PTY E2E + snapshots: assert tool call visibility in golden path; update snapshots for new UI text

  **What to do**:
  - Update `crates/harness-testkit/tests/pty_e2e.rs`:
    - Extend `pty_e2e_tui_golden_path` to assert that the screen contains a tool call marker (e.g., `edit.hashline_apply` or `toolcall_` id) after permission approval.
    - Capture a new visual checkpoint focusing on the Activity pane showing tool call entry.
    - Update insta snapshots accordingly (`crates/harness-testkit/tests/snapshots/*`).
  - Ensure all PTY tests still run deterministically under CI env vars.

  **Must NOT do**:
  - Do NOT weaken assertions to vague strings; use specific, stable markers.

  **Recommended Agent Profile**:
  - Category: `visual-engineering`
  - Skills: []

  **Parallelization**: Can Parallel: NO | Wave 4 (final) | Blocks: - | Blocked By: 17,18

  **References**:
  - Golden path PTY test structure: `crates/harness-testkit/tests/pty_e2e.rs:42-175`
  - Scenario runner performs tool call: `crates/harness/src/tui.rs:461-499`

  **Acceptance Criteria**:
  - [ ] `HARNESS_DETERMINISTIC=1 HARNESS_DISABLE_ANIMATIONS=1 TZ=UTC LANG=C.UTF-8 LC_ALL=C.UTF-8 TERM=xterm-256color RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: PTY run shows tool call in Activity
    Tool: cargo-mcp_cargo_test
    Steps:
      1) Run PTY E2E suite as in Acceptance Criteria
    Expected:
      - Snapshots updated; tool call marker present; test passes.
    Evidence: .sisyphus/evidence/task-19-pty-tool-visibility.txt
  ```

  **Commit**: YES | Message: `test(pty): assert tool call visibility in TUI` | Files: [`crates/harness-testkit/tests/pty_e2e.rs`, `crates/harness-testkit/tests/snapshots/*`]

- [ ] 20. Documentation + config examples: update docs to reflect tool calling + tool IDs + TUI config mode

  **What to do**:
  - Update docs:
    - `README.md` (interactive TUI uses config when present; tool calling overview)
    - `docs/architecture.md` (multi-turn loop + tool call lifecycle; artifacts policy)
    - `docs/config.md` (category.tools uses canonical tool ids, list built-in tools)
  - Update `configs/harness.example.jsonc`:
    - Include a realistic category (e.g., `deep`) toolset with canonical ids: `fs.read`, `fs.ls`, `fs.glob`, `fs.grep`, `edit.hashline_scan`, `edit.hashline_apply`, `shell.run`, `agent.delegate`.

  **Must NOT do**:
  - Do NOT claim compatibility with OpenCode or pi-agent-rust.

  **Recommended Agent Profile**:
  - Category: `writing`
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 (late) | Blocks: - | Blocked By: 1-19

  **References**:
  - Existing config→profiles mapping: `crates/harness/src/bootstrap.rs:72-99`
  - Existing README testing + TUI instructions.

  **Acceptance Criteria**:
  - [ ] `cargo test --workspace --all-features -- --test-threads=1`

  **QA Scenarios**:
  ```
  Scenario: Example config validates
    Tool: cargo-mcp_cargo_run
    Steps:
      1) Run: cargo run -p harness -- config validate --config configs/harness.example.jsonc
    Expected:
      - Validation succeeds.
    Evidence: .sisyphus/evidence/task-20-config-validate.txt
  ```

  **Commit**: YES | Message: `docs: milestone-1 tool calling and toolset configuration` | Files: [`README.md`, `docs/architecture.md`, `docs/config.md`, `configs/harness.example.jsonc`]


## Final Verification Wave (4 parallel agents, ALL must APPROVE)
- [ ] F1. Plan Compliance Audit — oracle
- [ ] F2. Code Quality Review — unspecified-high
- [ ] F3. Real Manual QA (Agent-executed) — unspecified-high (+ PTY E2E)
- [ ] F4. Scope Fidelity Check — deep

## Commit Strategy
- Atomic commits per TODO where feasible.
- Conventional messages: `feat(core): ...`, `feat(providers): ...`, `feat(tools): ...`, `feat(tui): ...`, `test(...): ...`, `docs: ...`.

## Success Criteria
- Milestone 1 Definition of Done commands pass.
- Interactive (config-based) and scenario-based flows remain deterministic under CI env vars.
- Tool calling works end-to-end: provider emits tool call → tool executes → result reinjected → assistant completes.

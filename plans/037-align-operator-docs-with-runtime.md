# Plan 037: Align operator documentation with active permissions and provider behavior

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- docs/tools/native-tool-catalog.md docs/permissions/privacy-and-local-data.md docs/configuration/provider-support.md`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** IMPLEMENTED — source and whitespace checks pass; independent review pending.
- **Issue:** [#260](https://github.com/urbanbreach/agent-harness/issues/260)
- **Priority:** P2
- **Effort:** S
- **Risk:** LOW
- **Depends on:** plan 029 (`plans/029-keep-mock-model-picker-offline.md`, [issue #252](https://github.com/urbanbreach/agent-harness/issues/252))
- **Category:** docs
- **Audit finding:** 36 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

Operator documentation labels skill loading as task permission, describes outgoing traffic too narrowly, and presents an OpenAI-only runtime despite an Anthropic backend. These contradictions make permission and offline expectations unreliable. Correct the three existing documents against current executable mappings.

## Current state

`docs/tools/native-tool-catalog.md:29` — The skill row states the wrong permission category.

```markdown
| `session_list` | none | read-only | Replay-derived JSON | Model-visible session catalog listing with filters/sort/caps. |
| `session_read` | none | read-only | Replay-derived JSON; large output spills | Bounded redacted event/message windows. Supports `include_todos` and `from_end` params. |
| `session_search` | none | read-only | Replay-derived JSON; large output spills | Redacted search over safe replay-derived session text. |
| `shell.run` | `bash` | host command | Captured output and artifacts when large | Lower-level shell id kept canonical for compatibility tests. |
| `skill` | `task` | prompt/control-plane read | Summary plus loaded skill content | Loads configured markdown skills under skill permission rules. |
| `task` | `task` | child scheduling | Child session events and structured runtime metadata | Named subagent delegation. New tasks require `subagent_type`, `prompt`, `run_in_background`, and `load_skills`; `description` and `command` are optional, while `task_id`/`session_id` continue a direct child. There is no category selector. |
| `todoread` | `task` | read-only | Control-plane state output | Reads the run-local todo state. |
```

`docs/permissions/privacy-and-local-data.md:3` — The egress description omits automatic model-catalog refresh.

```markdown
Harness is local-first. It writes local sessions, artifacts, config, prompts, and skills, and it sends data out only through explicitly configured provider or MCP calls.

## Data egress

The only routine data egress paths are configured provider requests and enabled MCP server calls. `webfetch`, `websearch`, and `codesearch` are also explicit tool calls under permission policy. Replay, session inspection, doctor, and support export are local/offline unless an operator runs a live provider lane.
```

`crates/harness-core/src/perm.rs:848` — The executable mapping assigns skill to Read.

```rust
        "lsp" => Some(PermissionKind::Lsp),
        "lsp.rename" => Some(PermissionKind::EditFs),
        "bash" | "shell.run" => Some(PermissionKind::Shell),
        "read" | "skill" => Some(PermissionKind::Read),
        "edit" | "write" | "apply_patch" => Some(PermissionKind::EditFs),
        "github.issue" | "github.pull_request" => Some(PermissionKind::Network),
```

## Conventions and exemplar

Documentation only. Plan 029 makes mock TUI initialization embedded/offline; describe that guarantee only after it lands. Match actual bootstrap backend dispatch and catalog behavior without claiming that every catalog entry is executable or live-certified. Do not edit permission implementation or the actively maintained permissions.md.

`docs/configuration/provider-support.md:60` — Retain the existing concrete model-catalog source/cache controls, correcting contradictory surrounding claims.

```markdown

Use config/env-backed provider credentials. Missing credentials are reported without printing secret values. Invalid credentials and rate limits require live prompt evidence because doctor stays offline.

## Model catalog refresh

The bundled model catalog is refreshed from `https://models.dev/api.json` using a five-minute cache. Harness accepts both the direct models.dev provider map and the generated catalog shape, serves a valid stale cache immediately, and refreshes stale data in the background with an atomic, mode-`0600` cache write. Set `HARNESS_DISABLE_MODELS_FETCH=1` to keep the embedded catalog only; `HARNESS_MODELS_URL` and `HARNESS_MODELS_PATH` override the source and cache location.

For the built-in `openai-codex` provider, refreshed OpenAI model metadata is merged into the configured Codex model list without replacing explicit entries. This lets newly published GPT models appear in `/model` while preserving local variants and provider settings. Unknown live entries receive conservative metadata and the existing Codex model-id reasoning policy; a provider-specific model endpoint is not required for this catalog path.

## Resolved model limits

`ResolvedModelLimits` is the runtime authority for context, maximum input, and maximum output tokens. `max_input` means provider-visible input tokens before generated output; it is not a percentage or a request-budget calculation. Each field records whether it came from explicit configuration, the generated catalog, provider discovery, or a compatibility fallback, together with its source and optional verification date.

A selectable known model must provide positive context and output values, with output no larger than context. `max_input` is an independent optional physical provider cap; when present it must be positive and no larger than context, and when absent its value and provenance remain unknown. A custom model may omit all three fields, and Harness does not infer a window from the model family. Variants replace only the fields they explicitly set. `harness models` prints every field and its per-field provenance.

```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `git diff --check` | Expected results are specified per step; final run passes with nonzero selection. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `docs/tools/native-tool-catalog.md`
- `docs/permissions/privacy-and-local-data.md`
- `docs/configuration/provider-support.md`

Administrative updates to `plans/037-align-operator-docs-with-runtime.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-037-align-operator-docs-with-runtime` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Correct the native tool permission row

Change skill's category from task to read, following perm.rs's permission_kind_for_tool mapping. Retain the tool's prompt/control-plane purpose and describe its existing permission behavior without inventing a separate gate.

**Verify:** `git diff --check` → git diff --check passes; the skill row matches the executable mapping.

### Step 2: Describe the actual model-catalog egress path

Update privacy-and-local-data.md to include ordinary catalog initialization/refresh traffic, cache use, configured source and disable controls already present in provider_catalog.rs and provider-support.md. Preserve documented explicit provider/MCP/web calls and read-only replay/readiness guarantees. After plan 029, distinguish mock's embedded-only catalog source.

**Verify:** `rg -n 'catalog|MODELS|DISABLE|mock|offline' docs/permissions/privacy-and-local-data.md docs/configuration/provider-support.md crates/harness-core/src/provider_catalog.rs` → The privacy and provider-support descriptions agree on catalog egress and supported controls.

### Step 3: Match provider support to backend dispatch

Update provider-support.md's introductory support claim to reflect OpenAI-compatible and Anthropic backends selected in bootstrap.rs. Keep catalog availability separate from executable backend support and actual credential/live verification. Read all three changed documents together and run the whitespace check; no new tests or builds are needed for prose.

**Verify:** `git diff --check` → All three documents are internally consistent with the cited source, and git diff --check exits 0.

## Test plan

No new tests are needed for these prose corrections. Verify each claim against the named executable mapping and run the document checks above.

## Done criteria

All must hold:

- [x] The skill catalog row states read permission.
- [x] Privacy documentation names automatic catalog refresh and the actual cache/source/disable behavior.
- [x] Provider support names implemented backend families without equating catalog presence with live support.
- [x] Only the three documentation files and plan status records change; git diff --check passes.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- Plan 029 has not established mock's embedded-only path; omit that unimplemented guarantee and wait to finalize it.
- Runtime mappings or backend dispatch have changed since the baseline; reconcile the text against the actual behavior.

## Maintenance notes

Permission mappings, new backend dispatch branches and catalog network-policy changes must update these same operator documents.

## Execution evidence — 2026-09-20

- Updated only the three permitted operator documents and this plan record.
  The plan 029 change in the same branch provides the mock catalog guarantee
  before this documentation commit.
- Verified `skill` against `permission_kind_for_tool` in `perm.rs`: `Read`.
- Verified implemented provider families against `build_provider` in
  `bootstrap.rs`: `OpenAiCompatible` and `Anthropic`. Catalog presence is kept
  separate from transport implementation, credentials, and live verification.
- Verified catalog text against `ProviderCatalog::cached`/`from_env`, cache TTL,
  `models_url`, `models_path`, and `fetch_disabled` in `provider_catalog.rs`.
  Documents agree on ordinary catalog traffic, five-minute cache freshness,
  stale-cache background refresh, embedded fallback, source/cache overrides,
  the fetch-disable switch, and mock's embedded-only initialization.
- `rg -n 'catalog|MODELS|DISABLE|mock|offline'
  docs/permissions/privacy-and-local-data.md
  docs/configuration/provider-support.md crates/harness-core/src/provider_catalog.rs`:
  source and documentation claims agree.
- Read all three updated documents together; `git diff --check`: exit 0.
  No new tests or builds are needed for these prose corrections.
- Independent review and the index update in `plans/README.md` are delegated to
  the integrating operator before issue closure.

# Permissions guide and V1 threat model

Harness permissions are an operator approval layer, not a sandbox. They decide whether the coordinator may run a requested tool action; they do not confine the operating system, container, shell, provider, or editor once an operator approves a dangerous action.

## Permission names

The public V1 permission names are:

- `bash`
- `edit`
- `question`
- `task`
- `webfetch`
- `websearch`
- `codesearch`
- `lsp`
- `read`
- `external_directory`
- `doom_loop`

Legacy internal names such as shell/network may appear in compatibility code, but docs, prompts, and public config should use the V1 names above.

## Allow, ask, deny

`allow` lets the coordinator run the tool without another prompt. `ask` records a permission request and waits for operator approval. `deny` blocks the tool before execution and records the denial. Profile overrides, defaults, and selector rules are resolved by the coordinator before any native tool code runs.

## Allow-by-default product model

The canonical scalar form is OpenCode-aligned allow-by-default:

```jsonc
{ "permission": "allow" }
```

A scalar applies to all canonical public permission kinds. When `permission` is omitted, ordinary tools default to allow while safety kinds stay guarded: `external_directory` and `doom_loop` default to ask, and `read` defaults to allow with targeted `.env` pattern asks.

This is intentionally not a full OpenCode PermissionNext engine. Harness uses a dual Policy plus ruleset seam: permission resolution returns allow/ask/deny, and the runtime still applies tool-level capability checks and tool-specific safety gates afterward. A permission allow does not bypass workspace path validation, shell safety parsing, or the doom-loop streak counter.

## Mutable surfaces

Approving `edit` can change workspace files. Approving `bash` can run host commands inside the configured workspace and can indirectly mutate files; bash approvals may be scoped to reusable command patterns such as `cargo nextest run *`. Approving `task` can spawn child agents or control background work. Network permissions (`webfetch`, `websearch`, `codesearch`) can send request data to configured services. `question` can interrupt the operator flow. `lsp` can inspect code and, through rename-capable routes, may require edit permission for mutations.

## Targeted safety asks

Three mechanisms ask for extra operator confirmation even when the base permission is allow:

| Kind | Trigger | Default | Notes |
|---|---|---|---|
| `read` `.env` patterns | Reading a file whose basename matches `*.env` or `*.env.*` | ask | `*.env.example` is explicitly allowed. Paths outside the workspace raise `external_directory` instead. |
| `external_directory` | Any tool argument that resolves outside the configured workspace | ask | Grant-gated: an approved ask can record a call-scoped prefix so later calls under the same path do not re-ask until the run ends. Bash path-like tokens that the shell scanner misses are denied, not allowed. |
| `doom_loop` | The third identical call to the same tool with the same arguments | ask | Streak is counted per `(tool_id, permission_request_digest)` on the run. `allow` with mode `once` resets the streak; `always` marks the run as always-granted so the kind no longer asks. |

There is no OpenCode-style temporary-directory whitelist. Workspace-relative paths and explicit call-scoped grants are the only supported escape gates.

## Runtime-enforced vs behavioral promises

The runtime-enforced vs behavioral split is explicit:

| Promise | Enforced by runtime? | Notes |
|---|---|---|
| Tool availability per profile | yes | Coordinator filters toolsets before execution. |
| Catch-all deny hides tools from the model | yes | Provider tool lists omit tools whose last matching permission rule is `pattern: "*"` + `action: deny` (Harness `disabled` / `visibleTools`). Partial path/command allows keep the tool visible. |
| Permission decision before execution | yes | Permission policy returns allow/ask/deny before tool code runs. |
| Plan may edit only the active plan file | yes | Plan tools are constrained by runtime policy and plan handoff tools. |
| Explore denies edit/task | yes for configured tools | Explore allows read/search plus bash/webfetch/websearch (ruleset-compatible); edit, task, and codesearch stay denied. |
| Category routes deny recursion | yes by default config | Category profiles deny recursive `task` unless config changes. |
| Bash globs and `/dev/null` | yes (permission-patterns mode) | Shell globs and safe device redirects are not hard-blocked as workspace escapes; true out-of-workspace paths still fail closed. |
| Ask one question / keep responses concise | behavioral | Prompt guidance only; not a sandbox. |
| Prefer small changes and manual QA | behavioral | Verified by review/tests, not by permission policy. |

## Agent matrix summary

Effective defaults by shipped profile:

| Profile | Notable allow | Notable ask | Notable deny |
|---|---|---|---|
| `build` | ordinary tools | `external_directory`, `doom_loop` | — |
| `plan` | `question`, `plan_exit`, plan-file `edit` | `bash` (read-only guard), `external_directory`, `doom_loop` | `plan_enter`, non-explore `task`, non-plan `edit` |
| `general` | ordinary tools, `task` | `external_directory`, `doom_loop` | `question`, `plan_enter`, `plan_exit`, `todowrite` |
| `explore` | `read`, `glob`, `grep`, `list`, `bash`, `webfetch`, `websearch` | `external_directory`, `.env` read | `edit`, `task`, `question`, `codesearch`, `plan_enter`, `plan_exit` |
| category routes | ordinary tools | `external_directory`, `doom_loop` | `task` (recursive delegation) |

Intentional Harness divergences from the OpenCode agent.ts matrix:

- `plan.shell` is ask plus a read-only bash guard, not allow.
- Category routes deny `task` by default to block recursive delegation.
- Plan edit allow targets `.agent-harness/plans/*` instead of `.opencode/plans/*.md`.

## Ruleset-compatible evaluation

Rules are ordered; **last match wins**. When no rule matches a permission+pattern pair, the default action is **ask**. Config scalars (`permission.bash: "allow"`) expand to `pattern: "*"`. Pattern maps (`permission.bash: { "git *": "allow", "*": "ask" }`) expand one rule per entry.

Selector-capable kinds are `bash`, `edit`, `task`, `read`, and `external_directory`. Scalar-only kinds are `question`, `webfetch`, `websearch`, `codesearch`, `lsp`, and `doom_loop`.

Task tool descriptions list only subagents that are not denied under the caller's task permission rules.

## Residual risks

Permission policy improves operator UX by deciding whether a tool call runs, asks, or is denied. It is not a sandbox or security boundary.

- A `bash` command that contains a path-like token the shell safety scanner does not recognize is denied, not allowed.
- `external_directory` grants are call-scoped prefixes on the run; they are not persisted session grants or a global whitelist.
- Harness does not implement an OpenCode temporary-directory whitelist.

## Fixture cross-link

WS6 couples these promises to runtime behavior through the permission-promise fixture: prompt-claimed restrictions for plan, explore, general, and category routes must match coordinator-denied tools. Harness permission parity inventory lives under `crates/harness-core/tests/fixtures/permission_ruleset_parity/` with progress in `docs/permissions-ruleset-parity-progress.md`.

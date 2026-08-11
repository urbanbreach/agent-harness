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

Scalar `ask`/`deny` paint every canonical public kind. Scalar `allow` (and the omitted-permission default) is allow-with-safety-exceptions: ordinary tools default to allow while safety kinds stay guarded — `external_directory` and `doom_loop` stay ask, base `question` stays deny until a named profile explicitly allows it, and `read` defaults to allow with targeted `.env` pattern asks.

This is intentionally not a full OpenCode PermissionNext engine. Harness uses a dual Policy plus ruleset seam: permission resolution returns allow/ask/deny, and the runtime still applies tool-level capability checks and tool-specific safety gates afterward. A permission allow does not bypass workspace path validation, shell safety parsing, or the doom-loop streak counter.

## Mutable surfaces

Approving `edit` can change workspace files. Approving `bash` can run host commands inside the configured workspace and can indirectly mutate files; bash approvals may be scoped to reusable command patterns such as `cargo nextest run *`. Approving `task` can spawn child agents or control background work. Network permissions (`webfetch`, `websearch`, `codesearch`) can send request data to configured services. `question` can interrupt the operator flow. `lsp` can inspect code and, through rename-capable routes, may require edit permission for mutations.

## Targeted safety asks

Three mechanisms ask for extra operator confirmation even when the base permission is allow:

| Kind | Trigger | Default | Notes |
|---|---|---|---|
| `read` `.env` patterns | Reading a file whose basename matches `*.env` or `*.env.*` | ask | `*.env.example` is explicitly allowed. Paths outside the workspace raise `external_directory` instead. |
| `external_directory` | Any tool argument, including bash `cwd`/`workdir` and path-like `--option=value` values, that resolves outside the configured workspace | ask | Grant-gated: an approved ask can record a call-scoped prefix so later calls under the same path do not re-ask until the run ends. Bash path-like tokens that the shell scanner misses are denied, not allowed. |
| `doom_loop` | The third identical call to the same tool with the same arguments | ask | Streak is counted per `(tool_id, permission_request_digest)` on the run. `allow` with mode `once` resets the streak; `always` marks the run as always-granted so the kind no longer asks. |

There is no OpenCode-style temporary-directory whitelist. Workspace-relative paths and explicit call-scoped grants are the only supported escape gates.

## Runtime-enforced vs behavioral promises

The runtime-enforced vs behavioral split is explicit:

| Promise | Enforced by runtime? | Notes |
|---|---|---|
| Tool availability for the generic agent | yes | Coordinator filters the singleton toolset before execution. |
| Catch-all deny hides tools from the model | yes | Provider tool lists omit tools whose last matching permission rule is `pattern: "*"` + `action: deny` (Harness `disabled` / `visibleTools`). Partial path/command allows keep the tool visible. |
| Permission decision before execution | yes | Permission policy returns allow/ask/deny before tool code runs. |
| Bash globs and `/dev/null` | yes (permission-patterns mode) | Shell globs and safe device redirects are not hard-blocked as workspace escapes; true out-of-workspace paths still fail closed. |
| Ask one question / keep responses concise | behavioral | Prompt guidance only; not a sandbox. |
| Prefer small changes and manual QA | behavioral | Verified by review/tests, not by permission policy. |

## Generic agent summary

The generic agent uses the configured top-level permission policy plus its optional singleton `agent.permission` overlay:

| Execution | Notable allow | Notable ask | Notable deny |
|---|---|---|---|
| `default` | configured ordinary tools, including `task` when enabled | `external_directory`, `doom_loop`, and any operator-configured asks | any capability denied by the effective policy |

The generic parent and each named subagent have explicit toolsets and permission overlays. Worker capability filtering and direct-child ownership prevent delegation bypasses.

## Ruleset-compatible evaluation

Rules are ordered; **last match wins**. When no rule matches a permission+pattern pair, the default action is **ask**. Config scalars (`permission.bash: "allow"`) expand to `pattern: "*"`. Pattern maps (`permission.bash: { "git *": "allow", "*": "ask" }`) expand one rule per entry.

Selector-capable kinds are `bash`, `edit`, `task`, `read`, and `external_directory`. Scalar-only kinds are `question`, `webfetch`, `websearch`, `codesearch`, `lsp`, and `doom_loop`.

The task tool selects a named `subagent_type` and has no category router. Task permission is evaluated before every child start or continuation.

## Residual risks

Permission policy improves operator UX by deciding whether a tool call runs, asks, or is denied. It is not a sandbox or security boundary.

- A `bash` command that contains a path-like token the shell safety scanner does not recognize is denied, not allowed.
- `external_directory` grants are call-scoped prefixes on the run; they are not persisted session grants or a global whitelist.
- Harness does not implement an OpenCode temporary-directory whitelist.
- Approved interpreter code (`python3 -c`, heredocs, and equivalent forms) can perform host I/O that lexical shell path scanning cannot enumerate; use an enforced OS sandbox when shell approval must remain filesystem-confined.
- **Folder trust** (workspace allow/deny for repository-local executables) and **OS sandbox policy** (process confinement intent + availability) are separate runtime layers from permission allow/ask/deny. Approving `bash` does not grant folder trust or claim OS sandbox success when enforcement is unavailable.

## Fixture cross-link

Permission-policy tests couple these promises to runtime behavior: denied task calls never spawn children, worker restrictions remain enforced, and provider tool lists omit catch-all-denied tools.

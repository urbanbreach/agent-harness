# Permissions

Harness permissions are an operator approval layer, not a sandbox. They decide
whether the coordinator may run a tool. An approved shell command can still
perform host I/O beyond what the shell path scanner can identify.

## Permission names

| Permission | Controls |
| --- | --- |
| `bash` | Shell commands and stdio MCP capabilities |
| `edit` | Native file changes, including LSP rename |
| `question` | Questions sent to the operator |
| `task` | Child tasks and background task controls |
| `webfetch` | Fetching remote content |
| `websearch` | Web search |
| `codesearch` | Code search and native AST search |
| `lsp` | Language server queries |
| `read` | File and skill reads |
| `external_directory` | Access outside the configured workspace |
| `doom_loop` | Repeated identical calls |

Use these names in new configs. `shell` and broad `network` names remain
compatibility inputs.

## Allow, ask, deny

The coordinator resolves defaults, role overrides, and selector rules before
tool execution. `allow` proceeds without another prompt. `ask` records a request
and waits for approval. `deny` records a denial and stops the call.

A grant can satisfy an ask but cannot override a deny. An allowed call still has
to pass tool availability, workspace path checks, shell parsing, and any
tool-specific validation.

## Defaults

```jsonc
{ "permission": "allow" }
```

Scalar `allow`, or omitting `permission`, allows ordinary tools with safety
exceptions. `external_directory` and `doom_loop` stay at ask. `read` asks for
sensitive `.env` patterns. Base `question` stays denied until a named profile
allows it. Scalar `ask` and `deny` apply to every public kind.

| Extra check | Trigger | Behavior |
| --- | --- | --- |
| Sensitive file read | The effective basename matches `*.env` or `*.env.*` | Ask; `*.env.example` is allowed. External paths use the external-directory check. |
| External directory | A tool argument resolves outside the workspace, including bash `cwd`, `workdir`, or path-like `--option=value` arguments | Ask; an approval can grant a path prefix for later calls in the current run. |
| Repeated call | A third consecutive call has the same tool ID and permission-request digest | Ask; allowing once resets the streak, while allowing always suppresses later asks for the run. A child deny still wins. |

External-directory grants are run-local path prefixes, not global or persisted
session grants. There is no automatic temporary-directory whitelist. Unrecognized
path-like shell tokens are denied.

## Pattern-rule evaluation

Rules keep their authored JSON/JSONC order. The last matching rule wins. A
permission and pattern pair with no matching rule defaults to ask.

```jsonc
{
  "permission": {
    "bash": {
      "*": "deny",
      "git status*": "allow",
      "cargo nextest run*": "ask"
    },
    "edit": {
      "*": "ask",
      "docs/**": "allow"
    }
  }
}
```

Per-kind scalars such as `"bash": "allow"` expand to a catch-all rule. Selector
maps are supported for `bash`, `edit`, `task`, `read`, and `external_directory`.
The other kinds accept scalars only.

Later config layers retain inherited patterns, then append their own in authored
order. An overridden pattern moves to the later position. A scalar replaces that
kind's pattern map; omitted kinds inherit it. Legacy rule arrays replace earlier
arrays and preserve their explicit order. Named-agent rules and the `shell`
alias follow the same ordering.

Older loaders sorted pattern keys. If a config relied on that behavior, reorder
it explicitly. `{ "*": "allow", "git status": "deny" }` denies `git status`;
reversing the entries allows it.

## File targets and grants

File rules check both the normalized requested path and its effective target,
including symlinks and creation paths beneath existing directories. A deny on
either wins. Otherwise, an ask on either remains an ask. Invalid or unresolvable
paths fail closed.

Sensitive-file checks also follow effective targets, including in always-approve
mode. Reusable grants bind aliases to those targets. Pending approvals revalidate
them before execution. External paths retain their separate check. These checks
do not provide race-free filesystem confinement or per-file filtering inside a
broad directory search.

## Parent and child agents

The parent uses shared policy plus `agent.default.permission`. Each child has
its own toolset and role policy. Parent tool membership and `task` permission
control starts and continuations. A parent's edit deny does not transfer to a
child whose role permits editing.

For every child action, combine shared policy with the child's role policy,
without the parent's overlay. Deny wins, then ask, then allow. Remembered grants
and always-approve cannot override a child deny. Batch inner calls use the same
membership and permission checks.

`explore` and `librarian` have research tools, bash, LSP, skills, and discovered
MCP tools. They lack native editing, `lsp.rename`, questions, tasks, and todo
mutation. `general` adds editing and receives skills through `load_skills`.
Research prompts prohibit implementation, but bash and MCP can still mutate files.

Discovery adds concrete MCP tools to the parent and research roles. General
requires exact registered IDs in its tool list. Stdio MCP uses `bash`; HTTP MCP
uses network policy. Transport type and read-only hints do not establish safety.
Discovery does not add generic MCP gateways.

Skills supply instructions and cannot grant tools or permissions. Loading uses
read permission and the per-skill policy. Resume uses current configuration and
the same preparation checks as a new child, without rewriting history. See the
[role tool lists and delegation diagram](../operations/generic-agent-and-tasks.md#permission-and-toolset-boundaries).

## Runtime checks and prompt guidance

The runtime-enforced vs behavioral distinction matters when evaluating a policy:

| Rule | Enforcement |
| --- | --- |
| Tool membership and permission decisions | The coordinator checks them before execution. |
| Catch-all deny hides a tool | Provider tool lists omit it. Partial selector allowances keep it visible and trigger argument checks. |
| Workspace paths and shell syntax | Tool validation checks them after permission resolution. Permission-patterns mode accepts globs and safe `/dev/null` redirects. |
| Concise answers, research-only work, small changes | Prompts guide the model. Review and tests check the result. |

## Residual risks

Approving `edit` permits file changes. Approving `bash` permits commands that can
change files indirectly. Network tools can send data to configured services.
Use OS-enforced isolation when approved commands must remain confined.

Folder trust separately controls repository-local executables. OS sandbox policy
separately describes confinement and whether enforcement is available. A bash
approval grants neither folder trust nor proof of a working sandbox.

Permission tests check that denied tasks never spawn, worker restrictions hold,
and catch-all-denied tools disappear from provider tool lists.

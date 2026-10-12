# Permissions

The coordinator checks tool membership and policy before execution. Permissions
are application approval checks. They do not confine an approved process at the
operating-system boundary.

## Rules and defaults

Rules use `allow`, `ask` or `deny`. The last matching rule wins within each policy.
Shared policy and the acting agent's role policy then combine: deny wins, followed
by ask, then allow. A grant or YOLO mode cannot override a deny.

Public permission names are `bash`, `edit`, `read`, `eval`, `network`, `question`,
`task`, `webfetch`, `websearch`, `codesearch`, `lsp`, `external_directory`, and
`doom_loop`. The legacy `shell` configuration name maps to `bash`.

| Tool or operation | Permission checks |
| --- | --- |
| `read`, `glob`, `grep`, `list` | `read` on the requested path |
| `edit`, `write`, `apply_patch` | `edit` on affected paths |
| Structural search or replacement | `read`, plus `edit` when applying changes |
| `bash` | `bash` on the command and parsed command prefixes |
| `eval` and eval kernel tools | `eval` |
| `spawn_subagent` or `task` | The tool ID and `task` on the selected child type |
| `lsp` | `lsp` and `read`; installation decisions check `lsp` only |
| LSP rename | `lsp` and `read`, plus `edit` when `apply` is true |
| `webfetch`, `websearch`, `codesearch` | Their own IDs, not a shared `network` switch |
| Session, skill and other native tools | Their own IDs unless the tool supplies specific checks |
| MCP | Transport permission, server operation ID and discovered tool ID |

Stdio MCP transport checks `bash` against its configured command. HTTP transport
checks `network` against its endpoint. A discovered MCP call also checks its
tool ID against serialized arguments and `mcp.<server>.tool.call` against the
remote tool name. Generic MCP calls enforce the discovered tool check too.
Permission checks run before lazy connection or tool execution.

Omitting `permission`, or using scalar `allow`, allows ordinary operations with
these defaults:

- Outside-workspace paths and repeated identical calls require approval.
- Reads matching `*.env` or `*.env.*` require approval; `*.env.example` is allowed.
- Questions are denied by the shared default; enabling them also requires the acting role to permit them.

Scalar `ask` or `deny` sets defaults across the public permission kinds. Existing
read rules still take precedence, including the built-in environment-file rules.
Use a `read` pattern map to change those rules. Pattern maps are available for
bash, edit, task, read and external directories:

```jsonc
{
  "permission": {
    "bash": { "*": "deny", "git status*": "allow" },
    "edit": { "*": "ask", "docs/**": "allow" }
  }
}
```

Patterns match the entire selector. `*` matches any characters, including path
separators, and `?` matches one character. Other characters are literal; these
are not regular expressions or shell glob character classes.

JSON/JSONC source order is significant. Later configuration layers append new
patterns after inherited entries. Replacing an existing pattern keeps its position;
a scalar replaces that kind's map. Legacy rule arrays retain their explicit order.
Explicit pattern rules run after per-kind defaults. If no rule or configured
default matches, the coordinator asks.

A `permission` object without a `"*"` key has no catch-all, so tools that have
no permission kind of their own (`todowrite`, `skill`, the session tools) ask on
every call. Start the object with `"*": "allow"` to keep them allowed. It does
not loosen the built-in asks for outside paths and repeated calls, or the
question deny.

Tools denied for every argument are omitted from provider tool lists. A partial
allowance keeps the tool visible; execution still checks its actual arguments.

## Approvals and remembered grants

The CLI presets change defaults for `edit`, `bash` and `network` only. They do
not remove explicit pattern rules, role restrictions, sensitive-read rules or
the external-directory and repetition checks. `--sandbox` is a permission
preset, not OS confinement.

| Flag value | Edit | Bash | Network |
| --- | --- | --- | --- |
| `--sandbox readonly` (or `read-only`) | deny | deny | deny |
| `--sandbox workspace` | ask | ask | deny |
| `--sandbox danger` (or `full`) | allow | allow | allow |
| `--permission-mode default` | ask | ask | ask |
| `--permission-mode acceptEdits` | allow | ask | ask |
| `--permission-mode dontAsk` | deny | deny | deny |
| `--permission-mode yolo` or `--yolo` | allow | allow | allow |

`--permission-mode` overrides `--sandbox`; `--yolo` overrides both and enables
session YOLO mode. `--allow` and `--deny` then set individual defaults. Each
accepts comma-separated names, with deny applied after allow. Accepted names
are `edit` (aliases `write`, `patch`), `bash` (alias `shell`), `network`,
`question`, `task`, `read`, `webfetch`, `websearch`, `codesearch`, `lsp`,
`external_directory` (alias `external`) and `doom_loop` (alias `doom`).
They accept permission kinds, not command patterns or arbitrary tool IDs.
`eval` is configured through `permission.eval`, not these two flags.

An ask commits a permission request and waits. A configured timeout expires as a
denial. Cancellation resolves waiting approvals and prevents the tool from
starting. Enabling YOLO releases eligible pending requests and handles
future ordinary asks. Questions, sensitive reads, repeated-call checks and
outside-workspace access keep their own approval requirements.

`harness --yolo` starts with YOLO enabled. The TUI shows the active mode
in the composer. Mode changes are recorded in the session journal,
so resuming the session restores the last choice. Use `/yolo` or Ctrl+O to toggle
it. New sessions use the launch flag or configuration rather than another session's
choice.

Remembered approvals have run, session or workspace scope. Run grants end with
the run. Session grants restore from that session's journal. Workspace
grants use a private `<project>/.harness/permission-grants.json` file. Tools cannot
edit that file or managed session storage.

The matcher depends on the approved operation:

| Operation | Remembered match |
| --- | --- |
| One unaliased workspace file | Later calls of that tool on the same resolved file, with different arguments |
| Aliased paths, multiple targets, or a dynamic refactoring request | The exact request digest |
| Bash | The same command or approved parsed command prefixes; every command in a list or pipeline must match |
| Outside-workspace paths | The requested directory, or a requested file's parent directory, across tools; checked by path components, never a bare root prefix |
| Repeated-call approval | Future repetition checks, subject to the acting role's deny rules |

External approvals are separate from a tool's ordinary approval. A saved file or
shell grant cannot approve a new outside directory. Remembered directory access
applies across tools within that directory. Each tool still passes its ordinary
permission checks, and deny rules still win. File and shell grants remain tied
to the approved tool. Expiring grants are not reused.

A third consecutive identical tool request triggers `doom_loop`. Allowing once
resets the streak. Remembering that approval suppresses later repetition asks;
a child role's denial still wins. The streak resets when the run ends.
YOLO alone does not disable this guard.

There are at most 4,096 retained grants. Workspace storage is limited to 1 MiB.
Grant descriptions that would expose a registered secret use an opaque request
digest instead. Large shell matchers also fall back to a digest.

## Paths and delegated work

File rules check the requested path and its resolved target. A deny on either
wins. Native tools bind approved arguments to those targets and reject a changed
symlink when resolving the path for use. Refactoring tools approve newly discovered
targets before editing them. Broad searches omit files needing another read
approval. These checks do not provide race-free filesystem confinement.

The caller needs the `spawn_subagent` tool and `task` permission to start a child
or resume completed child context.
Native subagents copy the parent's role permission rules into their registration
and remain subject to the shared project policy. Their tool selection is limited
by the parent's available toolset and the child definition. Nested eval tool calls
use the same coordinator checks. Skills provide instructions, not authority.
See [agents and tasks](../operations/generic-agent-and-tasks.md).

Approving bash permits host commands within the configured shell parser and
allowlist. Network tools can transmit data to their configured services. Optional
[OS confinement](../tools/shell.md) remains separate from operator approval.
For the bash tool, `HARNESS_OS_SANDBOX_POLICY` opts into `workspace_write`,
`read_only` or `strict`; the default is `off`. Non-off policies require Linux
Landlock filesystem ABI 5 and `/usr/bin/setpriv`. They restrict the launched
shell's filesystem access, not networking, and do not confine eval. An unavailable
confinement mechanism fails the shell launch rather than silently running it
without confinement.

## Eval permission

`permission.eval` accepts `allow`, `ask`, or `deny` and defaults to `allow`, like
`bash`. Eval permits arbitrary local code, imports, subprocesses, and direct
filesystem/network access from a persistent interpreter. It is not a sandbox.
Remove eval from the toolset or deny `permission.eval` when arbitrary local code
is not acceptable. Calls through `tool.<name>` still use the caller's normal
toolset and permission checks; an eval approval
does not grant any nested tool permission. See [eval](../tools/eval.md).

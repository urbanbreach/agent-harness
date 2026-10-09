# Extension strategy

Harness supports config-backed MCP servers, Markdown skills, Markdown prompt
commands and native lifecycle hooks. Executable extension command hooks remain
unsupported.

| Extension | What it can do | Where to configure it |
| --- | --- | --- |
| MCP server | Register concrete tools from an enabled server | `mcp` in runtime config |
| Markdown skill | Add instructions and declared resources when activated | Configured skill roots |
| Markdown prompt command | Expand a slash command into an agent prompt | Project `.harness/commands/` or `<home>/commands/` |
| Native lifecycle hook | Run an allowed command at a coordinator lifecycle point | Runtime hook config |

## Config-backed MCP

MCP servers are declared in runtime config under `mcp`. Enabled servers register discovered tools through the native registry. Disabled entries remain documented but do not launch.

## Markdown skills

Harness ships no skill pack. Default project roots are `.harness/skills` and
`.agents/skills`; default global roots are `<home>/skills` and
`$HOME/.agents/skills`. Here `<home>` is `HARNESS_HOME` when nonempty, otherwise
`~/.harness`.
Discovery reads frontmatter and compact metadata; full bodies and declared
`resources` load only when the `skill` tool or a subagent definition preload
activates them. Skills never grant runtime tools or bypass coordinator
permissions. See the [skill contract](../configuration/config.md#skill-discovery-and-v1-skill-contract)
for discovery order, frontmatter, and activation rules.

Bundled resources use progressive disclosure. The `resources` frontmatter field
is a comma- or newline-separated list of relative file paths under the skill
directory. Directories, globs, absolute paths, `..`, and symlink escapes are
rejected before reading. V1 caps each activation to 5 files, 64 KiB per file, 200
KiB total loaded bytes, and path depth 4 under the skill root. Loaded resource
text is redacted and appended to the normal skill activation body. The catalog,
doctor, and support output expose compact metadata only.

Harness skill roots rank above `.agents/skills` and `~/.agents/skills`. Duplicate
skills show as `shadowed` in the catalog. Other assistant roots such as
`.claude/skills`, `.external-editor/skills` and `.assistant/skills` are not
searched by default; list them in `skills.project_roots` or
`skills.global_roots` to import them.

## Markdown prompt commands

Add `*.md` files under `<project>/.harness/commands/` or `<home>/commands/`.
Discovery searches the current directory and ancestors up to the nearest Git
root, nearest first, then user commands, then bundled commands. Outside Git,
only the current project directory is searched. The first matching name wins.
A command's name is its file stem and must match `[a-z0-9][a-z0-9_-]*`.
Built-in TUI command names and aliases remain reserved. Conflicting files are
skipped with a warning on CLI stderr or a TUI warning toast.

YAML frontmatter accepts `description` and `argument-hint`; the Markdown body
is the prompt template. For example, `.harness/commands/explain.md`:

```markdown
---
description: Explain a file for a new contributor
argument-hint: '[file] [question]'
---
Read $1 and explain it in the context of this repository.
Answer this question: $2
```

`$ARGUMENTS` and `$@` insert the whole trimmed argument string. `$1` through
`$9` insert quote-aware positional arguments; missing positions expand to an
empty string. With no placeholders, nonempty arguments are appended after a
blank line. Expansion is one pass, so inserted text is not expanded again.

Use `/explain "src/main.rs" "How does startup work?"` in the TUI, or:

```bash
harness run '/explain "src/main.rs" "How does startup work?"'
harness prompt '/explain "src/main.rs" "How does startup work?"'
```

TUI completion shows descriptions and argument hints; custom commands also
appear in the command palette. They expand into prompts, not shell commands,
and do not grant tools or bypass permissions. The bundled `/init` asks the
agent to inspect the repository and create or update a concise root
`AGENTS.md`, preserving existing user content. Project or user commands can
override that template.

## Native lifecycle hooks

Configure commands in `hooks.lifecycle`. Each entry chooses an event, an argument
vector, an optional workspace-relative `cwd`, a timeout, a `critical` flag, and
explicit environment values. See [hook execution](hooks.md) for an example,
phase order, limits, and failure handling.

The coordinator runs hooks in configuration order. Critical failures veto the
owning operation; noncritical failures produce a warning. Commands must appear
exactly in `permissions.shell_allowlist.executables`. Hook output cannot approve
a tool, modify a provider response, or supply a compaction summary.

Replay, recovery of historical events, and inspection never execute hooks.
Deterministic execution records skipped task receipts. Resuming a session invokes
only the new run's lifecycle hooks.

Built-in slash commands perform UI actions. Markdown slash commands expand into
agent prompts; they do not directly execute shell commands.

## Skill activation and state

Skill discovery does not write event logs or activate skill bodies. Activation
changes request prompt context through the `skill` tool or a subagent
definition's `skills` list, subject to coordinator permission checks. The
existing event schema and tool output summaries record that activity. Readiness
and support output contain compact catalog metadata, never full skill bodies.

## Unsupported extension execution

Runtime extension package loading, executable extension command hooks,
manifest-driven MCP launch, provider decorators, and extension-provided tool
registration are not supported. MCP tools, skills, Markdown prompt commands and
lifecycle hooks use the paths above. Lifecycle hooks run through the coordinator.

Executable plugins, upstream plugin compatibility, browser and media automation,
OAuth MCP, server hosting, session sharing, enterprise administration, cloud
services, telemetry, and billing remain post-V1.

## Verification requirements

Document each supported extension, its doctor output, its permission checks, and
its failure behavior. Verify those contracts with deterministic tests before
claiming release support.

# Harness documentation

New here? The [README](../README.md) gets you from install to a first session in a
few minutes, and the [Finnish README](README.fi.md) covers the same ground. The
guides below go deeper.

## Set up

| Guide | What it covers |
| --- | --- |
| [Log in to a provider](operations/providers.md) | Sign-in flows, API keys, Claude accounts and `harness auth` |
| [Provider support](configuration/provider-support.md) | Transports, credential sources, default models, limits and errors |
| [Configuration](configuration/config.md) | Every runtime and keyboard key, config layers and precedence |
| [Inspect configuration and models](operations/inspection.md) | `doctor`, `config`, `models`, `memory`, `code-graph` and `attribution` |
| [Troubleshooting](operations/troubleshooting.md) | Config, sign-in, provider, tool and recovery failures |

## Work with the agent

| Guide | What it covers |
| --- | --- |
| [Run a prompt](operations/prompt.md) | `harness run` and `harness prompt`: input, models, tools, permissions, resume and output |
| [Agents and tasks](operations/generic-agent-and-tasks.md) | Built-in and custom subagents, model roles, delegation limits and skills |
| [Permissions](permissions/permissions.md) | Rule syntax and order, presets, approvals, remembered grants and shell confinement |
| [Saved sessions](operations/sessions.md) | Listing, search, branching, rewind, recovery, import and exports |
| [Sessions and replay](architecture/sessions-and-replay.md) | How journals, resume, forks and offline replay work, and legacy storage |
| [Workspace data](operations/workspace-data.md) | Memory, code index, edit attribution, worktrees and input queues |
| [Privacy and local data](permissions/privacy-and-local-data.md) | What is stored where, what leaves your machine and how exports are redacted |

## Tools

| Guide | What it covers |
| --- | --- |
| [Native tools](tools/native-tool-catalog.md) | Every built-in tool, its permission key, limits and prerequisites |
| [Shell commands](tools/shell.md) | Foreground and background commands, timeouts, output and Landlock confinement |
| [Monitor](tools/monitor.md) | Background command and file watches, event limits and rearm |
| [Eval](tools/eval.md) | Persistent JavaScript and Python kernels and calling tools from code |
| [Language tools](tools/lsp.md) | LSP servers, diagnostics, navigation and renames |
| [Structural search and replacement](tools/ast-grep-replace-safety-gate.md) | ast-grep search and the checks before a rewrite touches files |
| [File formatting](tools/formatting.md) | Formatter discovery, overrides, edit receipts and cancellation |
| [Web tools](tools/web.md) | Fetch and search, limits and providers |
| [GitHub tools](tools/github.md) | Issues and pull requests through the GitHub API |
| [MCP tools](tools/mcp.md) | Stdio and HTTP servers, discovery, pagination and permissions |
| [Session tools](tools/sessions.md) | Letting the agent list, read and search past sessions |

## Extend Harness

| Guide | What it covers |
| --- | --- |
| [Extensions](operations/extension-strategy.md) | Skills, MCP servers, `AGENTS.md` loading and Markdown slash commands |
| [Lifecycle hooks](operations/hooks.md) | Hook events, configuration, limits and failure handling |
| [Agent prompts](../crates/harness-core/prompts/README.md) | Bundled system prompts, per-model presets, overrides and template bindings |
| [Stdio peer diagnostic](operations/operators.md) | `harness agent`, `harness worktree` and other operator commands |
| [Migration notes](operations/migration-notes.md) | Storage moves, unsupported commands and removed prototype APIs |

## Work on Harness

| Guide | What it covers |
| --- | --- |
| [Runtime architecture](architecture/architecture.md) | Crate boundaries, coordinator ownership and event flow |
| [Terminal design](../DESIGN.md) | Layout, themes, keys, motion and accessibility |
| [Testing](testing/testing.md) | Test lanes, suite ownership and evidence requirements |
| [Performance budgets](testing/budgets.md) | Measured limits and the release-mode checks behind them |
| [Build performance](testing/build-performance.md) | Build profiles, linkers and measured build times |
| [Releasing](operations/releasing.md) | Cutting a release, the release workflow, assets and recovery |
| [README media](assets/README.md) | How the demo animation and still image are recorded |

## Design and audit records

These record decisions and measurements at a specific revision. They are kept as
history and are not updated when the code moves on. Use the guides above for
current behavior.

- Engine: [inventory](architecture/engine-inventory.md),
  [target design](architecture/engine-target.md),
  [migration](architecture/engine-migration.md) and
  [backend rewrite](architecture/backend-rewrite.md).
- Terminal UI: [TUI rewrite](tui-rewrite.md), [UI polish](ui-polish-2026-09-15.md),
  [settings polish](ui-settings-polish-2026-09-15.md),
  [settings regression](tui-current-settings-regression.md),
  [presentation parity](presentation-parity-implementation.md),
  [chat rendering](chat-tool-render-parity.md) and
  [alignment](chat-alignment-tool-arguments.md).
- Subagents: [native subagent rewrite](subagent-parity.md).
- Measurements: [backend rewrite](performance/backend-rewrite-2026-09-26.md),
  [TUI fluidity](performance/tui-fluidity-2026-09-13.md),
  [Rust 2024 migration](performance/rust-2024-2026-10-05.md),
  [eval](performance/eval-2026-10-05.md),
  [eval code mode](performance/eval-codemode-2026-10-05.md) and
  [Node eval](performance/eval-node-2026-10-05.md).
- [Archived verification evidence](evidence/README.md).

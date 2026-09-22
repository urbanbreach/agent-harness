# Harness documentation

Start with the [README](../README.md) to build Harness and try it offline.
The [Finnish README](README.fi.md) covers the same setup.

## Use Harness

| Guide | What it covers |
| --- | --- |
| [Configuration](configuration/config.md) | Runtime keys, keyboard settings, model selection, and config precedence |
| [Providers](configuration/provider-support.md) | Implemented transports, credentials, model limits, and errors |
| [Agents and tasks](operations/generic-agent-and-tasks.md) | Parent and child roles, delegation, and skill loading |
| [Permissions](permissions/permissions.md) | Rule order, approval prompts, shared policy, and limits |
| [Native tools](tools/native-tool-catalog.md) | Tool IDs, permissions, output, and prerequisites |
| [Sessions and replay](architecture/sessions-and-replay.md) | Inspection, resume, branching, and support exports |
| [Privacy and local data](permissions/privacy-and-local-data.md) | Storage, outgoing requests, and redaction |
| [Troubleshooting](operations/troubleshooting.md) | Config, authentication, tools, and recovery failures |
| [Starter skills](configuration/starter-skills.md) | Bundled skill instructions and activation rules |
| [Extensions](operations/extension-strategy.md) | Skills, MCP, hooks, and manifest descriptors |
| [Migration notes](operations/migration-notes.md) | Unsupported commands and removed prototype APIs |

## Develop Harness

| Guide | What it covers |
| --- | --- |
| [Architecture](architecture/architecture.md) | Crate boundaries, runtime ownership, and event flow |
| [Terminal design](../DESIGN.md) | Layout, color, input, motion, and accessibility |
| [Testing](testing/testing.md) | Test commands, suite ownership, and evidence requirements |
| [Performance budgets](testing/budgets.md) | Measured limits and release-mode checks |
| [Structural edit safety](tools/ast-grep-replace-safety-gate.md) | Validation before AST replacements change files |
| [README media](assets/README.md) | How to regenerate the animation and still image |

## Design and audit records

These documents record decisions and measurements at a particular revision. Use
the guides above for current commands and configuration.

- Engine changes: [inventory](architecture/engine-inventory.md),
  [target design](architecture/engine-target.md), and [migration](architecture/engine-migration.md).
- Terminal changes: [UI polish](ui-polish-2026-09-15.md),
  [settings](ui-settings-polish-2026-09-15.md),
  [implementation record](grok-build-parity-implementation.md),
  [chat rendering](chat-tool-render-parity.md), and [alignment](chat-alignment-tool-arguments.md).
- Measurements: [TUI fluidity](performance/tui-fluidity-2026-09-13.md) and
  [performance audit](../plans/performance-audit/README.md).
- [Implementation plans and delivery records](../plans/README.md).

# Migration notes

V1 supports local coding through the CLI and TUI. The table lists product areas
that are unavailable, followed by changes to internal prototype APIs.

## Unsupported by design for V1

| Area | V1 stance |
|---|---|
| HTTP server | Unavailable in V1; no hosted API/server mode ships here. |
| web share | Unavailable in V1; local support export is the V1 path. |
| plugin host | Typed extension manifest descriptors ship for V1; runtime plugin hosting remains post-V1. |
| background autoupdate | Unavailable; operators can apply a validated local update with the [update command](operators.md). |
| enterprise | Explicitly post-V1. |
| desktop/mobile/PWA | Outside V1. |
| browser/media automation | Outside V1. |
| OAuth MCP | Unavailable in V1; configure supported MCP transports directly. |
| remote collaboration bots | Outside V1. |
| Ralph/continuation loops | Autonomous continuation loops are not a V1 runtime feature. |

## Supported migration path

Move local-coding workflows onto the Harness CLI/TUI, event store, provider config, markdown skills, and native tool registry. Keep unsupported upstream product areas inactive in config. Check the [config reference](../configuration/config.md) before carrying over
compatibility keys. Some accepted keys have no runtime effect.

## Evidence rule

A compatibility claim needs an implementation, documentation, and a passing
deterministic or explicitly enabled live check.

## Backend replacement

The backend was replaced while retaining the terminal source and its public
contracts. Existing working CLI commands, provider protocols, permissions,
append-only sessions and tools remain supported. Removed backend tests were
replaced with checks through those public boundaries. See the
[rewrite report](../architecture/backend-rewrite.md) for scope and verification,
and [testing](../testing/testing.md) for the current gates.

## Earlier prototype API cleanup

This earlier cleanup predates the backend rewrite. The names below describe that
revision and explain retired APIs; they are not a map of the replacement backend.

- Core `browser_oidc_local`, `mcp_oauth_local`, and `workspace_hub_local` simulators are removed; real authentication and workspace/session operations remain. No existing data files are deleted.
- Unconfigured `browser_oidc` and `mcp_oauth` workflow/probe APIs and removed hosted-hub operation APIs are retired, along with their outcome-only TUI fields. Availability APIs and `browser_oidc::launch_browser` remain; provider-specific authentication and configured MCP transports are unchanged.
- The disconnected `dashboard_dispatch` subsystem and `DashboardIntegration::dispatch_reply` are removed. Dashboard controls, selection, focus, and live composer routing remain.
- Core session, scheduler, workspace, and integration leaf adapters are removed; use `CoordinatorHandle` directly for the same coordinator-owned operations.
- The provider `leaf` factory is removed; runtime construction remains in CLI bootstrap using the concrete provider configurations.
- The unused TUI `slash` catalog is removed; `keybindings::slash_commands()` remains authoritative.
- Unobserved TUI media queues, contextual-tip state, performance samples, and shadow lifecycle transitions are removed. Actual transcript rendering, terminal titles/notifications, input bounds, and `lifecycle_choreography::LifecycleState` remain.
- The unused file-backed ACP echo transport (`integrations::acp_file`) is removed; stdio ACP and its connection lifecycle remain.
- Unconnected `jujutsu::jj_*` workflow wrappers and `JujutsuWorkflowResult` are removed; Jujutsu detection and diagnostic commands remain.
- The test-only plugin execution framework (`PluginExecutionSurface`, its sample plugins, and execution/cancellation methods and events) is removed. Descriptor install, activation permissions, persistence, and transactional upgrade/rollback remain unchanged.

The earlier cleanup also removed the arrange/act/assert comment convention gate.
Current backend gates check size, formatting, isolation and explicit native/live
opt-in; they do not require comment markers.

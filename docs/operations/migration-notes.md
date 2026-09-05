# Migration notes

Harness borrows lessons from source-inspiration projects while intentionally not shipping several product areas for V1.

## Unsupported by design for V1

| Area | V1 stance |
|---|---|
| HTTP server | Explicitly post-V1; no hosted API/server mode ships here. |
| web share | Explicitly post-V1; local support export is the V1 path. |
| plugin host | Typed extension manifest descriptors ship for V1; runtime plugin hosting remains post-V1. |
| autoupdate | Explicitly post-V1; builds are operator-controlled. |
| enterprise | Explicitly post-V1. |
| desktop/mobile/PWA | Non-goal for V1. |
| browser/media automation | Non-goal for V1. |
| OAuth MCP | Post-V1; config-backed MCP is the current safe seam. |
| remote collaboration bots | Non-goal for V1. |
| Ralph/continuation loops | Autonomous continuation loops are not a V1 runtime feature. |

## Supported migration path

Move local-coding workflows onto the Harness CLI/TUI, event store, provider config, markdown skills, and native tool registry. Keep unsupported upstream product areas inactive in config. Where compatibility keys are accepted, they are inert unless the V1 config reference states otherwise.

## Evidence rule

Do not claim upstream compatibility unless the behavior is implemented, documented, and covered by deterministic or live-gated evidence.

## Prototype API cleanup

The CLI, configuration, durable event schema, and active runtime behavior are unchanged. Unused or
unobserved Rust prototype APIs have been removed instead of maintaining parallel implementations:

- Core `browser_oidc_local`, `mcp_oauth_local`, and `workspace_hub_local` simulators are removed; real authentication and workspace/session operations remain. No existing data files are deleted.
- Core session, scheduler, workspace, and integration leaf adapters are removed; use `CoordinatorHandle` directly for the same coordinator-owned operations.
- The provider `leaf` factory is removed; runtime construction remains in CLI bootstrap using the concrete provider configurations.
- The unused TUI `slash` catalog is removed; `keybindings::slash_commands()` remains authoritative.
- Unobserved TUI media queues, contextual-tip state, performance samples, and shadow lifecycle transitions are removed. Actual transcript rendering, terminal titles/notifications, input bounds, and `lifecycle_choreography::LifecycleState` remain.
- The unused file-backed ACP echo transport (`integrations::acp_file`) is removed; stdio ACP and its connection lifecycle remain.
- Unconnected `jujutsu::jj_*` workflow wrappers and `JujutsuWorkflowResult` are removed; Jujutsu detection and diagnostic commands remain.
- The test-only plugin execution framework (`PluginExecutionSurface`, its sample plugins, and execution/cancellation methods and events) is removed. Descriptor install, activation permissions, persistence, and transactional upgrade/rollback remain unchanged.

Tests no longer require arrange/act/assert comment markers; the `conventions` gate and its empty
baseline have been removed. All other static test gates remain enabled.

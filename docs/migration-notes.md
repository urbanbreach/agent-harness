# Migration notes

Harness borrows lessons from source-inspiration projects while intentionally not shipping several product areas for V1.

## Unsupported by design for V1

| Area | V1 stance |
|---|---|
| HTTP server | Explicitly post-V1; no hosted API/server mode ships here. |
| web share | Explicitly post-V1; local support export is the V1 path. |
| plugin host | Typed extension manifest and plugin hosting are final-slice/post-V1. |
| autoupdate | Explicitly post-V1; builds are operator-controlled. |
| enterprise | Explicitly post-V1. |
| desktop/mobile/PWA | Non-goal for V1. |
| browser/media automation | Non-goal for V1. |
| OAuth MCP | Post-V1; config-backed MCP is the current safe seam. |
| remote collaboration bots | Non-goal for V1. |
| Team Mode | Only primitive event-sourced team surfaces ship; full Team Mode is post-V1. |
| Ralph/continuation loops | Autonomous continuation loops are not a V1 runtime feature. |

## Supported migration path

Move local-coding workflows onto the Harness CLI/TUI, event store, provider config, markdown skills, and native tool registry. Keep unsupported upstream product areas inactive in config. Where compatibility keys are accepted, they are inert unless the V1 config reference states otherwise.

## Evidence rule

Do not claim upstream compatibility unless the behavior is implemented, documented, and covered by deterministic or live-gated evidence.

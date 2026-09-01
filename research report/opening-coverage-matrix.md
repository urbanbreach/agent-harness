# Opening-harvest coverage matrix

Verified independently after DAG completion.

| Axis | Verdict | Bilateral evidence | Synthesis constraint |
|---|---|---|---|
| Shell | PARTIAL | Harness `ui.rs:208`; Grok `app/app_view.rs:221` | Adopt full-surface hierarchy, preserve Harness viewport contract. |
| Navigation | PARTIAL | Harness `app/lifecycle.rs:20`; Grok `views/dashboard/state.rs:275` | Compose Grok timeline/wayfinding with Harness operator rail. |
| Transcript | PARTIAL | Harness `ui_transcript_block_grammar_normalize.rs:6`; Grok `scrollback/state/groups.rs:20` | Use one authoritative fold projection; aggregation absence was refuted. |
| Rich content | PARTIAL | Harness `ui_markdown_table.rs:11`; Grok markdown `render.rs:1493` | Preserve Harness diff renderer; add wrapping/link/citation/open-fence behavior. |
| Composer | PARTIAL | Harness `app/prompt_input.rs:483`; Grok `agent_view/prompt.rs:830` | Separate cancel-and-replace from non-cancelling interject. |
| Commands | PARTIAL | Harness `slash/commands.rs:26`; Grok `slash/registry.rs:300` | Preserve deterministic ranks; unify metadata/highlights/dynamic sources. |
| Tools/tasks | PARTIAL | Harness `app/task_pane.rs:4`; Grok `views/tasks_pane.rs:170` | Improve task pane/status grammar; aggregation gap is refuted. |
| Overlays | PARTIAL | Harness `ui_overlays.rs:684`; Grok `views/modal_window.rs:76` | Adopt reusable modal chrome while preserving better Harness palette ranking. |
| Theme | PARTIAL | Harness `theme.rs:1325`; Grok `theme/groknight.rs:18` | Fix token/runtime drift; preserve Harness accents and names. |
| Motion | PARTIAL | Harness `scheduling/runtime_wheel.rs:92`; Grok `input/mouse.rs:665` | Add fluid feedback without weakening global reduced motion. |
| Responsive | PARTIAL | Harness `transcript_integration/incremental/reflow.rs:13`; Grok `scrollback/state/mod.rs:1526` | Prototype visible-window estimates behind exact-anchor tests. |
| Accessibility | PARTIAL | Harness `app/motion.rs:13`; Grok `appearance/config.rs:392` | Keep Harness reduced-motion lead; add mouse/key-event policy controls. |
| Performance | PARTIAL | Harness `ui_transcript_layout.rs:363`; Grok `scrollback/state/layout.rs:1688` | Treat full-history versus visible-window measurement as architecture. |
| States | PARTIAL | Harness `app/session_projection.rs:1326`; Grok `app/error_display.rs:11` | Wire existing reconnect; replace substring errors with typed categories. |
| Branding | PARTIAL | Harness `startup_logo.rs:6`; Grok `views/welcome/logo.rs:10` | Animate Harness identity; never copy Grok marks or voice. |
| Verification | PARTIAL | Harness `terminal/frame_output/backend.rs:51`; Grok PTY harness `screen.rs:34` | Add emulator-backed durable evidence bundles. |

## Independent proof

- The verifier read exactly the four allowed ledgers and returned 16 rows, five guardrails, and an `## EXPAND` tail.
- The lead parsed all 16 rows and re-opened all 32 cited Harness/Grok source anchors; every file and line was readable.
- DAG generation 4 settled at 64 completed, 0 failed, 0 cancelled, 0 skipped.
- The matrix is PARTIAL by design: “coverage exists” is not “parity exists.” The gaps advance to expansion.

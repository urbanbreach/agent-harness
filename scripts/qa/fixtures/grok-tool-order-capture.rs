// Production ACP tracker + native ScrollbackPane. No product source is copied or patched.
use agent_client_protocol as acp;
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::{CrosstermBackend, TestBackend},
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::StatefulWidget,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use xai_grok_pager::{
    acp::{meta::NotificationMeta, tracker::AcpUpdateTracker},
    scrollback::{
        block::RenderBlock,
        blocks::{
            SubagentBlock,
            tool::{HookPhase, HookRunEntry, HookRunStatus},
        },
        entry::EntryId,
        scrollback_pane::ScrollbackPane,
        state::ScrollbackState,
    },
    theme::{Theme, ThemeKind},
};
use xai_grok_tools::types::output::{
    BashOutput, FileContent, ReadFileOutput, ToolOutput, WebSearchOutput,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// The reference has no clock dependency injection. Linux executable-symbol interposition keeps
// its real Instant::now/elapsed code, including the completion flash test, on an exact clock.
// CLOCK_REALTIME is frozen too (entry timestamps); other clocks go to the real kernel syscall.
// The executable tests that Instant actually observes this injection before producing any frame.
static INJECT: AtomicBool = AtomicBool::new(false);
static CLOCK_MS: AtomicU64 = AtomicU64::new(0);
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[unsafe(no_mangle)]
unsafe extern "C" fn clock_gettime(id: libc::clockid_t, out: *mut libc::timespec) -> libc::c_int {
    if INJECT.load(Ordering::SeqCst) && (id == libc::CLOCK_MONOTONIC || id == libc::CLOCK_REALTIME)
    {
        let ms = CLOCK_MS.load(Ordering::SeqCst);
        let base = if id == libc::CLOCK_REALTIME {
            1_789_084_800
        } else {
            1_000
        };
        // SAFETY: clock_gettime's C ABI requires a writable timespec pointer.
        unsafe {
            out.write(libc::timespec {
                tv_sec: base + (ms / 1000) as i64,
                tv_nsec: ((ms % 1000) * 1_000_000) as i64,
            });
        }
        0
    } else {
        // SAFETY: forward the unchanged clock id and caller-owned pointer to the Linux syscall.
        unsafe { libc::syscall(libc::SYS_clock_gettime, id, out) as libc::c_int }
    }
}
fn clock(ms: u64) {
    CLOCK_MS.store(ms, Ordering::SeqCst);
}

struct Capture {
    tracker: AcpUpdateTracker,
    state: ScrollbackState,
    tools: BTreeMap<String, Value>,
    ids: BTreeMap<String, EntryId>,
    updates: Vec<(u64, acp::SessionUpdate)>,
    now_ms: u64,
    replay: bool,
    selected: bool,
    selection_anchor: Option<EntryId>,
}

impl Capture {
    fn new(fixture: &Value) -> Result<Self> {
        clock(0);
        let mut capture = Self {
            tracker: AcpUpdateTracker::new(),
            state: ScrollbackState::new(),
            tools: serde_json::from_value(fixture["tools"].clone())?,
            ids: BTreeMap::new(),
            updates: Vec::new(),
            now_ms: 0,
            replay: false,
            selected: false,
            selection_anchor: None,
        };
        capture.update(acp::SessionUpdate::UserMessageChunk(
            acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
                fixture["prompt"].as_str().ok_or("prompt")?,
            ))),
        ));
        Ok(capture)
    }

    fn update(&mut self, update: acp::SessionUpdate) {
        let meta = NotificationMeta {
            is_replay: self.replay,
            agent_timestamp_ms: Some(1_789_084_800_000 + self.now_ms as i64),
            stream_start_ms: Some(1_789_084_800_000),
            turn_start_ms: Some(1_789_084_800_000),
            ..Default::default()
        };
        self.tracker
            .handle_update(update.clone(), &meta, &mut self.state);
        if !self.replay {
            self.updates.push((self.now_ms, update));
        }
    }

    fn hooks(fixture: &Value, phase: &str) -> Result<Vec<HookRunEntry>> {
        fixture["hooks"]
            .as_array()
            .ok_or("hooks")?
            .iter()
            .filter(|h| h["phase"] == phase)
            .map(|h| {
                let elapsed = Duration::from_millis(h["duration_ms"].as_u64().ok_or("duration")?);
                let output = h["output"].as_str().map(str::to_owned);
                let status = match h["status"].as_str().ok_or("hook status")? {
                    "succeeded" => HookRunStatus::Success { elapsed },
                    "skipped" => HookRunStatus::Skipped,
                    "blocked" => HookRunStatus::Blocked {
                        detail: output.clone().ok_or("block detail")?,
                        elapsed,
                    },
                    "failed" => HookRunStatus::Failed {
                        error: output.clone().ok_or("error")?,
                        elapsed,
                    },
                    status => return Err(format!("bad hook status {status}").into()),
                };
                Ok(HookRunEntry {
                    name: h["name"].as_str().ok_or("hook name")?.into(),
                    // The shared fixture supplies one summary, not separate reason and stdout.
                    output: if matches!(
                        status,
                        HookRunStatus::Blocked { .. } | HookRunStatus::Failed { .. }
                    ) {
                        None
                    } else {
                        output
                    },
                    status,
                })
            })
            .collect()
    }

    fn output(tool: &Value) -> Result<Option<Value>> {
        let text = tool["output"].as_str().ok_or("output")?;
        let output = match tool["kind"].as_str().ok_or("kind")? {
            "read" => ToolOutput::ReadFile(ReadFileOutput::FileContent(FileContent {
                content: text.into(),
                content_concise: None,
                absolute_path: tool["args"]["path"].as_str().ok_or("path")?.into(),
                offset: None,
                limit: None,
                raw_output: text.into(),
                total_lines: text.lines().count(),
                extracted_images: Vec::new(),
            })),
            "execute" => ToolOutput::Bash(BashOutput {
                output: text.as_bytes().to_vec(),
                output_for_prompt: text.into(),
                exit_code: 0,
                command: tool["args"]["command"].as_str().ok_or("command")?.into(),
                truncated: false,
                signal: None,
                timed_out: false,
                description: None,
                current_dir: "/workspace".into(),
                output_file: String::new(),
                total_bytes: text.len(),
                output_delta: None,
                was_bare_echo: false,
            }),
            "search" => ToolOutput::WebSearch(WebSearchOutput {
                query: tool["args"]["query"].as_str().ok_or("query")?.into(),
                content: text.into(),
                citations: serde_json::from_value(tool["sources"].clone())?,
                allowed_domains: None,
                pre_formatted: None,
            }),
            _ => return Ok(None),
        };
        Ok(Some(serde_json::to_value(output)?))
    }

    fn action(&mut self, action: &Value, fixture: &Value) -> Result<()> {
        let id = action["id"].as_str().unwrap_or("");
        match action["op"].as_str().ok_or("missing op")? {
            "reasoning" => self.update(acp::SessionUpdate::AgentThoughtChunk(
                acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
                    action["text"].as_str().ok_or("reasoning text")?,
                ))),
            )),
            "text" => self.update(acp::SessionUpdate::AgentMessageChunk(
                acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
                    action["text"].as_str().ok_or("text")?,
                ))),
            )),
            "input" => {
                // The real XaiSessionUpdate::ToolCallDeltaChunk handler forwards name/index only;
                // argument bytes (valid OR malformed) never become scrollback placeholders.
                let before = self.state.len();
                self.tracker.note_tool_call_arguments_delta(
                    Some(self.tools[id]["tool"].as_str().ok_or("tool")?),
                    u32::try_from(action["index"].as_u64().ok_or("tool index")?)?,
                );
                assert_eq!(self.state.len(), before);
            }
            "request" => {
                let tool = &self.tools[id];
                let kind = match tool["kind"].as_str().ok_or("kind")? {
                    "read" => acp::ToolKind::Read,
                    "execute" => acp::ToolKind::Execute,
                    "search" => acp::ToolKind::Search,
                    "other" => acp::ToolKind::Other,
                    kind => return Err(format!("bad kind {kind}").into()),
                };
                let mut args = tool["args"].clone();
                if let Some(variant) = tool.get("variant") {
                    args["variant"] = variant.clone();
                }
                let tc = acp::ToolCall::new(
                    acp::ToolCallId::new(Arc::from(id)),
                    tool["title"].as_str().ok_or("title")?,
                )
                .kind(kind)
                .status(acp::ToolCallStatus::Pending)
                .raw_input(Some(args));
                self.update(acp::SessionUpdate::ToolCall(tc));
                if id == "task" {
                    // Task ToolCall is suppressed by ACP. This is the exported native constructor
                    // called by the private SubagentStarted handler, not a generic tool substitute.
                    let entry =
                        self.state
                            .push_block(RenderBlock::Subagent(SubagentBlock::started(
                                "Inspect renderer",
                                "child",
                                "general",
                                None,
                                None,
                                None,
                                true,
                            )));
                    self.ids.insert(id.into(), entry);
                } else {
                    self.ids.insert(
                        id.into(),
                        self.tracker
                            .pending_tool_entry_id(id)
                            .ok_or("missing typed pending entry")?,
                    );
                }
            }
            "start" => {
                if id == "task" {
                    self.state
                        .set_entry_running(*self.ids.get(id).ok_or("subagent entry")?, true);
                }
                self.update(acp::SessionUpdate::ToolCallUpdate(
                    acp::ToolCallUpdate::new(
                        acp::ToolCallId::new(Arc::from(id)),
                        acp::ToolCallUpdateFields::new()
                            .status(Some(acp::ToolCallStatus::InProgress)),
                    ),
                ));
            }
            "finish" => {
                let failed = action["failed"] == true;
                let tool = &self.tools[id];
                let text = if failed {
                    "Fixture read denied"
                } else {
                    tool["output"].as_str().ok_or("output")?
                };
                let fields = acp::ToolCallUpdateFields::new()
                    .status(Some(if failed {
                        acp::ToolCallStatus::Failed
                    } else {
                        acp::ToolCallStatus::Completed
                    }))
                    .raw_output(if failed { None } else { Self::output(tool)? })
                    .content(Some(vec![acp::ToolCallContent::Content(
                        acp::Content::new(acp::ContentBlock::Text(acp::TextContent::new(text))),
                    )]));
                let before = self.ids[id];
                self.update(acp::SessionUpdate::ToolCallUpdate(
                    acp::ToolCallUpdate::new(acp::ToolCallId::new(Arc::from(id)), fields),
                ));
                let entry = self
                    .state
                    .get_by_id(before)
                    .ok_or("completion replaced entry identity")?;
                assert!(!entry.is_running);
                assert_eq!(
                    entry
                        .finished_at
                        .ok_or("production completion did not stamp clock")?
                        .elapsed(),
                    Duration::ZERO
                );
                if action["hooks"] == true {
                    self.state.attach_hooks(
                        before,
                        HookPhase::Pre,
                        Self::hooks(fixture, "pre_tool_use")?,
                    );
                    self.state.attach_hooks(
                        before,
                        HookPhase::Post,
                        Self::hooks(fixture, "post_tool_use")?,
                    );
                }
            }
            "advance" => {
                let before = self.now_ms;
                self.now_ms += action["ms"].as_u64().ok_or("ms")?;
                clock(self.now_ms);
                for _ in before / 33..self.now_ms / 33 {
                    self.state.tick();
                }
            }
            "wait" => {
                assert!(self.state.set_pending_user_input(self.ids[id], true));
            }
            "unwait" => {
                assert!(self.state.set_pending_user_input(self.ids[id], false));
            }
            "open" | "fold" => {
                self.state
                    .set_selected(self.state.index_of_id(self.ids[id]));
                if action["op"] == "fold" {
                    self.state.toggle_fold_selected();
                    assert_eq!(
                        self.state
                            .get_by_id(self.ids[id])
                            .ok_or("fold entry")?
                            .display_mode,
                        xai_grok_pager::scrollback::types::DisplayMode::Truncated
                    );
                } else {
                    self.state.expand_selected();
                }
                self.state.clear_selection();
                self.selected = false;
            }
            "select" => {
                let mut index = self
                    .state
                    .index_of_id(self.ids[id])
                    .ok_or("selected tool")?;
                if self.state.entry_content_hidden_by_group(index) {
                    index = self.state.group_range_of(index, true).start;
                }
                self.selection_anchor = Some(self.state.get(index).ok_or("selected entry")?.id);
                self.state.set_selected(Some(index));
                self.selected = true;
            }
            "group" => {
                // Dense expansion clears selection. Reselect the same header for the reverse action.
                let anchor = self
                    .selection_anchor
                    .ok_or("group requires a selected anchor")?;
                self.state.set_selected(self.state.index_of_id(anchor));
                assert!(
                    self.state.toggle_group_expansion() || self.state.collapse_group_if_expanded(),
                    "no group at the selected anchor"
                );
            }
            "fold-selected" => self.state.toggle_fold_selected(),
            // Grok keeps compact rows when details are hidden; there is no success-row visibility toggle.
            "details-off" | "barrier" | "provider-finish" | "commit" => {}
            "dense" => {
                for i in 0..action["count"].as_u64().ok_or("count")? {
                    let id = format!("cmd-{i:02}");
                    self.tools.insert(id.clone(), json!({"tool":"bash", "kind":"execute",
                        "title":format!("Run command {i:02}"), "args":{"command":format!("printf command-{i:02}")},
                        "output":format!("command-{i:02}")}));
                    for op in ["request", "start", "finish"] {
                        self.action(&json!({"op":op, "id":id}), fixture)?;
                    }
                }
            }
            "replay" => {
                let encoded = serde_json::to_string(&self.updates)?;
                let updates: Vec<(u64, acp::SessionUpdate)> = serde_json::from_str(&encoded)?;
                let settled_ms = self.now_ms;
                self.state = ScrollbackState::new();
                self.tracker = AcpUpdateTracker::new();
                self.replay = true;
                for (ms, update) in updates {
                    self.now_ms = ms;
                    clock(ms);
                    self.update(update);
                }
                self.now_ms = settled_ms;
                clock(settled_ms);
                self.tracker.finish_turn(&mut self.state);
            }
            "lifecycle" => {
                self.state.push_lifecycle_hooks(
                    action["event"].as_str().ok_or("event")?.into(),
                    Self::hooks(fixture, "pre_tool_use")?,
                );
            }
            "cancel-task" => {
                self.state
                    .finish_running(*self.ids.get(id).ok_or("subagent entry")?);
                self.state
                    .push_block(RenderBlock::Subagent(SubagentBlock::cancelled(
                        "Inspect renderer",
                        "child",
                        Duration::from_millis(self.now_ms),
                    )));
            }
            op => return Err(format!("unknown op {op}").into()),
        }
        Ok(())
    }

    fn paint(&mut self, width: u16, height: u16, buffer: &mut Buffer) {
        let theme = Theme::current();
        let area = Rect::new(0, 0, width, height);
        buffer.set_style(
            area,
            Style::default().fg(theme.text_primary).bg(theme.bg_base),
        );
        let pane = Rect::new(2, 2, width - 4, height - 4);
        self.state.prepare_layout(pane.width, pane.height);
        ScrollbackPane::new()
            .active(self.selected)
            .render(pane, buffer, &mut self.state);
    }
    fn render(&mut self, width: u16, height: u16) -> Result<Buffer> {
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        terminal.draw(|frame| self.paint(width, height, frame.buffer_mut()))?;
        Ok(terminal.backend().buffer().clone())
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let fixture_path = args.next().ok_or("scenario JSON path required")?;
    let output = args.next().ok_or("output directory required")?;
    let output = Path::new(&output);
    fs::create_dir_all(output)?;
    INJECT.store(true, Ordering::SeqCst);
    clock(0);
    let epoch = Instant::now();
    clock(430);
    assert_eq!(
        epoch.elapsed(),
        Duration::from_millis(430),
        "clock interposition unavailable; no captures are valid"
    );
    assert_eq!(Instant::now(), Instant::now());
    let fixture: Value = serde_json::from_slice(&fs::read(fixture_path)?)?;
    Theme::apply_kind(ThemeKind::GrokNight);
    let mut captures = Vec::new();
    for scenario in fixture["scenarios"].as_array().ok_or("scenarios")? {
        let mut state = Capture::new(&fixture)?;
        for action in scenario["actions"].as_array().ok_or("actions")? {
            if action["op"] != "snapshot" {
                state.action(action, &fixture)?;
                continue;
            }
            for size in fixture["sizes"].as_array().ok_or("sizes")? {
                let width = size[0].as_u64().ok_or("width")? as u16;
                let height = size[1].as_u64().ok_or("height")? as u16;
                let name = format!(
                    "order-{}-{}-{width}x{height}-motion-{}ms",
                    scenario["name"].as_str().ok_or("name")?,
                    action["name"].as_str().ok_or("name")?,
                    state.now_ms
                );
                let buffer = state.render(width, height)?;
                assert_eq!(
                    buffer,
                    state.render(width, height)?,
                    "non-deterministic frame {name}"
                );
                let mut bytes = Vec::new();
                {
                    let mut terminal = Terminal::with_options(
                        CrosstermBackend::new(&mut bytes),
                        TerminalOptions {
                            viewport: Viewport::Fixed(Rect::new(0, 0, width, height)),
                        },
                    )?;
                    terminal.draw(|frame| state.paint(width, height, frame.buffer_mut()))?;
                }
                fs::write(output.join(format!("{name}.ansi")), bytes)?;
                let text = buffer
                    .content
                    .chunks(width as usize)
                    .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
                    .collect::<Vec<_>>()
                    .join("\n");
                fs::write(output.join(format!("{name}.txt")), text)?;
                captures.push(
                    json!({"name":name, "audit":action["audit"], "clock_ms":state.now_ms,
                    "entries":state.state.len(), "selected":state.state.selected()}),
                );
            }
        }
    }
    fs::write(
        output.join("producer.json"),
        serde_json::to_vec_pretty(&json!({
            "entrypoints":["AcpUpdateTracker::handle_update(SessionUpdate, &NotificationMeta, &mut ScrollbackState)",
                "AcpUpdateTracker::note_tool_call_arguments_delta", "ScrollbackState::attach_hooks", "ScrollbackState::push_lifecycle_hooks",
                "SubagentBlock::started/cancelled", "ScrollbackPane::render (StatefulWidget)"],
            "timing":{"mode":"Linux x86_64 executable clock_gettime interposition; asserted Instant delta 430ms; exact frozen frame clock",
                "tick_period_ms":33, "unit":"milliseconds"}, "captures":captures,
            "limitations":["private Xai notification handler bypassed only at its exported native hook/subagent/argument targets",
                "reference pane only; no fabricated full-app chrome or permission dock",
                "replay uses serialized SessionUpdate with NotificationMeta.is_replay, not malformed disk logs",
                "ordinary ACP ToolCallStatus has no cancellation variant"]
        }))?,
    )?;
    println!("Captured {} production reference frames", captures.len());
    Ok(())
}

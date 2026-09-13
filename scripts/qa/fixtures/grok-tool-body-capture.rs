// Production ACP ingestion followed by the native StatefulWidget pane.
// No block constructors: the tracker must select every specialized family.
use agent_client_protocol as acp;
use ratatui::{
    Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect, style::Style,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};
use xai_grok_pager::{
    acp::{meta::NotificationMeta, tracker::AcpUpdateTracker},
    scrollback::{
        ScrollbackPane, ScrollbackState,
        block::{BlockContent, RenderBlock},
        blocks::tool::ToolCallBlock,
    },
    theme::{Theme, ThemeKind},
};
use xai_grok_tools::types::output::ToolOutput;

fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn slug(s: &str) -> String {
    s.replace(['.', '_'], "-")
}
fn content(text: &str) -> Vec<acp::ToolCallContent> {
    vec![acp::ContentBlock::Text(acp::TextContent::new(text)).into()]
}
fn output_text(config: &Value, case: &Value) -> String {
    match string(case, "family") {
        "list" => string(case, "text").into(),
        "memory" if case["empty"] == true => "No memory results found.".into(),
        "memory" => (1..=3).map(|i| format!(
            "### Result {i} (score: 0.{}, source: workspace)\n**File:** memory/note-{i}.md (lines 10-14)\n```\nKeep the tool identity stable.\nA long memory snippet is bounded without losing the source range or score.\nThird snippet line.\nFourth snippet line.\n```", 90-i
        )).collect::<Vec<_>>().join("\n"),
        "mcp" => format!("**Issue ready**\nResource note: preserve the full tool response.\n{}", string(config, "long_text")),
        "media" if string(case, "mime") == "application/pdf" => "PDF read successfully".into(),
        "media" => "Image read successfully".into(),
        _ => case["text"].as_str().unwrap_or(string(config, "long_text")).into(),
    }
}

fn tool(config: &Value, case: &Value, state: &str) -> acp::ToolCall {
    let family = string(case, "family");
    let failed = state.starts_with("failure");
    let terminal = failed || state.starts_with("success");
    let path = string(case, "path");
    let text = output_text(config, case);
    let mut raw = None;
    let mut body = vec![];
    let (kind, title, input): (acp::ToolKind, String, Value) = match family {
        "execute" => {
            if terminal {
                raw = Some(bash_output(
                    if failed {
                        "Tool failed: deterministic capture error"
                    } else {
                        &text
                    },
                    failed,
                ));
            }
            (
                acp::ToolKind::Execute,
                "Run terminal inspection".into(),
                json!({"variant":"Bash", "command":"printf 'first\\nsecond\\n'\nprintf 'a deliberately long command argument for terminal reflow'", "description":"Inspect terminal output"}),
            )
        }
        "read" => {
            if terminal && !failed {
                raw = Some(json!({"type":"ReadFile", "FileContent":{
                    "content":text, "absolute_path":path, "offset":case["offset"].as_u64().unwrap()-1,
                    "limit":case["limit"], "raw_output":text, "total_lines":case["total"]
                }}));
            }
            (
                acp::ToolKind::Read,
                path.into(),
                json!({"variant":"ReadFile", "file_path":path, "offset":case["offset"].as_u64().unwrap()-1, "limit":case["limit"]}),
            )
        }
        "media" => {
            if terminal && !failed {
                raw = Some(if string(case, "mime") == "application/pdf" {
                    json!({"type":"ReadFile", "PdfPageImages":{"pages":[], "total_pages":case["pages"], "file_size":config["assets"][path]["bytes"]}})
                } else {
                    json!({"type":"ReadFile", "ImageContent":{"data":config["assets"][path]["base64"].as_str().unwrap_or(""), "mime_type":case["mime"]}})
                });
            }
            (
                acp::ToolKind::Read,
                path.into(),
                json!({"variant":"ReadFile", "file_path":path}),
            )
        }
        "edit" => {
            if terminal && !failed {
                let before = if string(case, "operation") == "create" {
                    ""
                } else {
                    string(config, "before")
                };
                body.push(
                    acp::Diff::new(path, string(config, "after"))
                        .old_text(Some(before.into()))
                        .into(),
                );
                if string(case, "operation") == "patch" {
                    body.push(
                        acp::Diff::new("src/second.rs", "pub const READY: bool = true;\n")
                            .old_text(Some(String::new()))
                            .into(),
                    );
                }
            }
            (
                acp::ToolKind::Edit,
                path.into(),
                json!({"variant":if string(case,"operation")=="create" {"Write"} else {"SearchReplace"}, "file_path":path}),
            )
        }
        "list" => {
            if terminal && !failed {
                raw = Some(
                    json!({"type":"ListDir", "Content":{"content":text, "absolute_root_path":path}}),
                );
            }
            (
                acp::ToolKind::Other,
                "List directory".into(),
                json!({"target_directory":path}),
            )
        }
        "search" => {
            let mode = string(case, "mode");
            if terminal && !failed {
                let stdout = if mode == "count" {
                    "src/renderer.rs:2\nsrc/layout.rs:1"
                } else {
                    "src/renderer.rs\nsrc/layout.rs"
                };
                raw = Some(
                    json!({"type":"GrepSearch", "stdout":stdout.as_bytes(), "stderr":[], "exit_code":0, "match_count":if mode=="files_with_matches" {2} else {3},
                        "file_matches": if mode == "content" { json!([
                            {"path":"src/renderer.rs", "matches":[{"line_number":41,"content":"ready: first match with enough explanatory text to exercise narrow wrapped output"},{"line_number":58,"content":"ready: second match"}]},
                            {"path":"src/layout.rs", "matches":[{"line_number":12,"content":"ready: layout match"}]}
                        ]) } else { json!([]) }
                    }),
                );
            }
            (
                acp::ToolKind::Search,
                "Search workspace".into(),
                if string(case, "id") == "glob" {
                    json!({"pattern":".", "glob":"**/*.rs", "path":"src", "output_mode":mode})
                } else {
                    json!({"pattern":"ready", "path":"src", "output_mode":mode})
                },
            )
        }
        "fetch" => {
            if terminal && !failed {
                raw = Some(
                    json!({"type":"WebFetch", "Content":{"url":"https://docs.example.org/terminal", "content":text, "content_type":"text/plain", "status_code":200, "bytes":text.len()}}),
                );
                body = content(&text);
            }
            (
                acp::ToolKind::Fetch,
                "Fetch: https://docs.example.org/terminal".into(),
                json!({"url":"https://docs.example.org/terminal"}),
            )
        }
        "web" => {
            if terminal && !failed {
                raw = Some(
                    json!({"type":"WebSearch", "query":"terminal reflow", "content":text, "citations":config["citations"], "allowed_domains":null}),
                );
            }
            (
                acp::ToolKind::Search,
                "Web search: terminal reflow".into(),
                json!({"variant":"WebSearch", "query":"terminal reflow"}),
            )
        }
        "mcp" => {
            if terminal {
                raw = Some(
                    json!({"type":"MCP", "tool_name":"save_issue", "server_name":"linear", "output":if failed {json!({"Error":"Tool failed: deterministic capture error"})} else {json!({"OkayOutput":text})}}),
                );
            }
            (
                acp::ToolKind::Other,
                "use_tool".into(),
                json!({"variant":"UseTool", "tool_name":"linear__save_issue", "tool_input":serde_json::from_str::<Value>(string(config, "mcp_arguments")).expect("shared MCP input fixture")}),
            )
        }
        "integration" => {
            if terminal && !failed {
                let result = if case["empty"] == true {
                    json!({"results":[]})
                } else {
                    json!({"results":[{"server":"linear","tools":[{"tool_name":"linear__save_issue","description":"Save an issue", "score":0.95}]},{"server":"calendar","tools":[{"tool_name":"calendar__find_event","description":"Find an event", "score":0.8}]}]})
                };
                raw = Some(
                    json!({"type":"SearchTool", "result_count":if case["empty"]==true {0} else {2}, "content":result.to_string()}),
                );
            }
            (
                acp::ToolKind::Other,
                "search_tool".into(),
                json!({"variant":"SearchTool", "query":"issue calendar", "limit":2}),
            )
        }
        "memory" => {
            if terminal && !failed {
                body = content(&text);
            }
            (
                acp::ToolKind::Other,
                "Memory search: \"stable tool identity\"".into(),
                json!({"query":"stable tool identity"}),
            )
        }
        "sent" => {
            if terminal {
                raw = Some(
                    json!({"type":"SendSubagentMessage", "outcome":if failed {"not_active_or_finalizing"} else {string(case,"outcome")}, "message_id":"message-1"}),
                );
            }
            (
                acp::ToolKind::Other,
                "send_subagent_message".into(),
                json!({"subagent_id":"child-capture", "text":"Inspect the tool body.\nKeep the original request and report the result.", "queue":false}),
            )
        }
        "unknown" => {
            if terminal && !failed {
                body = content(&text);
            }
            (
                acp::ToolKind::Other,
                "fixture.inspect".into(),
                json!({"query":"external result"}),
            )
        }
        _ => panic!("unhandled family: {family}"),
    };
    if failed && family != "mcp" {
        body = content("Tool failed: deterministic capture error");
    }
    // Reject incorrect typed wire data before asking the tracker to render it.
    if let Some(v) = &raw {
        serde_json::from_value::<ToolOutput>(v.clone()).expect("typed production ToolOutput");
    }
    acp::ToolCall::new("body-call", title)
        .kind(kind)
        .raw_input(Some(input))
        .raw_output(raw)
        .content(body)
        .status(if failed {
            acp::ToolCallStatus::Failed
        } else if terminal {
            acp::ToolCallStatus::Completed
        } else if state == "running" {
            acp::ToolCallStatus::InProgress
        } else {
            acp::ToolCallStatus::Pending
        })
}
fn bash_output(text: &str, failed: bool) -> Value {
    json!({"type":"Bash", "output":text.as_bytes(), "exit_code":if failed {1} else {0}, "command":"printf", "truncated":false, "signal":null, "timed_out":false, "description":null, "current_dir":".", "output_file":"", "total_bytes":text.len()})
}
fn variant(tc: &ToolCallBlock) -> &'static str {
    match tc {
        ToolCallBlock::Execute(_) => "execute",
        ToolCallBlock::Read(_) => "read",
        ToolCallBlock::Edit(_) => "edit",
        ToolCallBlock::Search(_) => "search",
        ToolCallBlock::ListDir(_) => "list",
        ToolCallBlock::WebFetch(_) => "fetch",
        ToolCallBlock::WebSearch(_) => "web",
        ToolCallBlock::UseTool(_) => "mcp",
        ToolCallBlock::IntegrationSearch(_) => "integration",
        ToolCallBlock::MemorySearch(_) => "memory",
        ToolCallBlock::SentMessage(_) => "sent",
        ToolCallBlock::Other(_) => "unknown",
        _ => panic!("unexpected tracker family"),
    }
}
fn freeze_clock(tc: &mut ToolCallBlock) {
    macro_rules! freeze {
        ($b:expr) => {{
            $b.started_at = None;
            $b.elapsed_ms = Some(0);
        }};
    }
    match tc {
        ToolCallBlock::Execute(b) => freeze!(b),
        ToolCallBlock::Read(b) => freeze!(b),
        ToolCallBlock::Edit(b) => freeze!(b),
        ToolCallBlock::Search(b) => freeze!(b),
        ToolCallBlock::ListDir(b) => freeze!(b),
        ToolCallBlock::WebFetch(b) => freeze!(b),
        ToolCallBlock::WebSearch(b) => freeze!(b),
        ToolCallBlock::UseTool(b) => freeze!(b),
        ToolCallBlock::IntegrationSearch(b) => freeze!(b),
        ToolCallBlock::MemorySearch(b) => freeze!(b),
        ToolCallBlock::SentMessage(b) => freeze!(b),
        ToolCallBlock::Other(b) => freeze!(b),
        _ => panic!("unexpected tracker clock"),
    }
}
fn capture(
    config: &Value,
    case: &Value,
    state: &str,
    width: u16,
    out: &Path,
    alias: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let height = config["height"].as_u64().unwrap() as u16;
    let mut tracker = AcpUpdateTracker::new();
    let mut sb = ScrollbackState::new();
    let meta = NotificationMeta {
        is_replay: true,
        agent_timestamp_ms: Some(1_000_000),
        ..Default::default()
    };
    tracker.handle_update(
        acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new("Inspect the tool body"),
        ))),
        &meta,
        &mut sb,
    );
    let pending = tool(config, case, "pending");
    assert!(tracker.handle_update(acp::SessionUpdate::ToolCall(pending), &meta, &mut sb));
    let final_call = tool(config, case, state);
    if state != "pending" {
        let fields = acp::ToolCallUpdateFields::new()
            .status(Some(final_call.status))
            .raw_output(final_call.raw_output.clone())
            .content(Some(final_call.content.clone()));
        assert!(tracker.handle_update(
            acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new("body-call", fields)),
            &meta,
            &mut sb
        ));
    }
    let index = sb.len() - 1;
    sb.prepare_layout(width - 4, height - 6);
    sb.set_selected(Some(index));
    if state != "success-default" {
        sb.collapse_selected();
    }
    if state.ends_with("open") || state == "success-member-closed" {
        // A non-foldable media/error member is still hidden by a singleton verb group.
        // Use the native group action to expose it before applying the native member action.
        if sb.is_selected_group_header() {
            assert!(sb.toggle_group_expansion());
            sb.set_selected(Some(index));
        }
        if state.ends_with("open") {
            sb.expand_selected();
        }
    }
    sb.clear_selection();
    let mut details = json!({"case":case["id"], "alias":alias, "state":state, "audit":config["producers"][string(case,"family")]["audit"], "wire":final_call, "clock":config["clock"]});
    for entry in sb.entries_mut() {
        entry.created_at = None;
        if entry.finished_at.is_some() {
            entry.finished_at = Some(Instant::now() - Duration::from_secs(60));
        }
        if let RenderBlock::ToolCall(tc) = &mut entry.block {
            assert_eq!(
                variant(tc),
                if string(case, "family") == "media" {
                    "read"
                } else {
                    string(case, "family")
                }
            );
            freeze_clock(tc);
            details["native_family"] = json!(variant(tc));
            details["foldable"] = json!(tc.is_foldable());
            details["mode"] = json!(format!("{:?}", entry.display_mode));
            details["image_refs"] = json!(tc.image_references().len());
            if let ToolCallBlock::Edit(edit) = tc {
                use xai_grok_pager::{
                    app::edit_highlight_worker::{EditHlJob, EditHlOutcome, spawn_worker},
                    scrollback::blocks::tool::EditHighlightPhase,
                };
                if matches!(state, "success-open" | "success-default") {
                    let after = string(config, "after");
                    let source = match string(case, "cap") {
                        "lines" => format!(
                            "{after}{}",
                            "// line\n".repeat(50_001 - after.lines().count())
                        ),
                        "bytes" => {
                            format!("{after}{}", "x".repeat(2 * 1024 * 1024 + 1 - after.len()))
                        }
                        _ => after.into(),
                    };
                    let source_dir = out.join("highlight-inputs");
                    fs::create_dir_all(&source_dir)?;
                    let source_path = source_dir.join(format!("{}.rs", string(case, "id")));
                    fs::write(&source_path, &source)?;
                    details["source_bytes"] = json!(source.len());
                    details["source_lines"] = json!(source.lines().count());
                    // Subscribe before submitting; await the worker's exact completion channel.
                    let (jobs, results) = spawn_worker();
                    jobs.send(EditHlJob {
                        job_id: 1,
                        entry_id: entry.id,
                        abs_path: source_path,
                        path: string(case, "path").into(),
                        hunks: edit.hunks.clone(),
                    })?;
                    let result = results.recv_timeout(Duration::from_secs(20))?;
                    assert_eq!(result.job_id, 1);
                    assert_eq!(result.entry_id, entry.id);
                    drop(jobs);
                    match result.outcome {
                        EditHlOutcome::Ready { by_new_line, theme } => {
                            assert!(case.get("cap").is_none());
                            details["highlight_worker"] = json!("Ready");
                            edit.highlight = EditHighlightPhase::FileScoped { by_new_line, theme };
                        }
                        EditHlOutcome::Failed => {
                            assert!(case.get("cap").is_some(), "ordinary source must highlight");
                            details["highlight_worker"] = json!("Failed: source cap");
                        }
                    }
                } else {
                    details["highlight_worker"] = json!("not submitted; hunk-only");
                }
            }
        }
        entry.invalidate_cache();
    }
    sb.invalidate_heights();
    // Match the Harness transcript viewport's two-cell outer inset, not its unrelated dock chrome.
    // The native pane retains all of its own layout and specialized body insets.
    let pane = Rect::new(2, if width >= 80 { 3 } else { 2 }, width - 4, height - 6);
    details["pane"] = json!({"x":pane.x,"y":pane.y,"width":pane.width,"height":pane.height});
    if width == 80 {
        sb.prepare_layout(116, pane.height);
    }
    sb.prepare_layout(pane.width, pane.height);
    sb.set_scroll_offset(0);
    let mut bytes = Vec::new();
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(&mut bytes),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, width, height)),
        },
    )?;
    terminal.draw(|frame| {
        let area = frame.area();
        let theme = Theme::current();
        frame.buffer_mut().set_style(
            area,
            Style::default().fg(theme.text_primary).bg(theme.bg_base),
        );
        frame.render_stateful_widget(ScrollbackPane::new(), pane, &mut sb);
    })?;
    drop(terminal);
    assert!(!bytes.is_empty());
    let name = format!(
        "body-{}-{}-{state}-{width}x{height}-motion-0ms",
        string(case, "id"),
        slug(alias)
    );
    fs::write(out.join(format!("{name}.ansi")), bytes)?;
    fs::write(
        out.join(format!("{name}.producer.json")),
        serde_json::to_vec_pretty(&details)?,
    )?;
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: harness_tool_body_capture SCENARIOS_JSON ANSI_DIR ASSET_ROOT".into());
    }
    let config: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    let out = fs::canonicalize(&args[2])?;
    std::env::set_current_dir(&args[3])?;
    Theme::apply_kind(ThemeKind::GrokNight);
    let mut count = 0;
    for case in config["cases"].as_array().unwrap() {
        for alias in case["aliases"].as_array().unwrap() {
            for width in config["widths"].as_array().unwrap() {
                for state in case
                    .get("states")
                    .unwrap_or(&config["states"])
                    .as_array()
                    .unwrap()
                {
                    // 80 columns is the representative opened-success reflow, not another lifecycle cross-product.
                    if width == 80 && state != "success-open" {
                        continue;
                    }
                    capture(
                        &config,
                        case,
                        state.as_str().unwrap(),
                        width.as_u64().unwrap() as u16,
                        &out,
                        alias.as_str().unwrap(),
                    )?;
                    count += 1;
                }
            }
        }
    }
    fs::write(
        out.join("producer.json"),
        serde_json::to_vec_pretty(&json!({
            "entrypoints":["AcpUpdateTracker::handle_update", "ScrollbackState::collapse_selected/expand_selected", "ScrollbackPane::render (StatefulWidget)", "edit_highlight_worker::spawn_worker (completion channel)"],
            "timing":config["clock"], "frames":count, "scope":"Production-shaped deterministic replay. Only timestamps and native worker result application are injected; no renderer substitution."
        }))?,
    )?;
    println!("Grok production tool bodies: {count} frames");
    Ok(())
}

//! Source-of-truth geometry captures from Grok Build's real block renderers.
use ratatui::{
    Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect, style::Style,
};
use std::{fs, path::Path, time::Duration};
use xai_grok_pager::{
    render::Renderable,
    scrollback::{
        block::RenderBlock,
        blocks::{
            SubagentBlock,
            tool::{
                EditToolCallBlock, ExecuteToolCallBlock, ListDirToolCallBlock, OtherToolCallBlock,
                ReadToolCallBlock, SearchFileMatch, SearchLineMatch, SearchOutputMode,
                SearchToolCallBlock, ToolCallBlock, WebSearchToolCallBlock,
            },
        },
        entry::ScrollbackEntry,
        types::DisplayMode,
        wrappers::EntryRenderer,
    },
    theme::{Theme, ThemeKind},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args().nth(1).ok_or("output directory required")?;
    let directory = Path::new(&directory);
    fs::create_dir_all(directory)?;
    Theme::apply_kind(ThemeKind::GrokNight);
    for (width, height) in [(40, 24), (80, 24), (120, 40)] {
        for family in [
            "task",
            "todowrite",
            "read",
            "write",
            "edit",
            "apply-patch",
            "bash",
            "grep",
            "glob",
            "list",
            "websearch",
            "mcp-fixture-inspect",
            "question",
        ] {
            for state in [
                "streaming",
                "queued",
                "waiting",
                "running",
                "running-tick",
                "succeeded",
                "succeeded-open",
                "succeeded-selected",
                "succeeded-closed",
                "failed",
                "failed-open",
                "failed-selected",
                "failed-closed",
            ] {
                let block = if state == "streaming" {
                    RenderBlock::ToolCall(ToolCallBlock::Other(OtherToolCallBlock::new("tool", "")))
                } else {
                    block(family, state)
                };
                let mut entry = ScrollbackEntry::new(block);
                entry.created_at = None;
                entry.is_running = state.starts_with("running") || state == "waiting";
                entry.is_pending_user_input = state == "waiting";
                entry.display_mode = if state.ends_with("open") || state.ends_with("selected") {
                    DisplayMode::Expanded
                } else {
                    DisplayMode::Collapsed
                };
                let mut bytes = Vec::new();
                let area = Rect::new(0, 0, width, height);
                let mut terminal = Terminal::with_options(
                    CrosstermBackend::new(&mut bytes),
                    TerminalOptions {
                        viewport: Viewport::Fixed(area),
                    },
                )?;
                terminal.draw(|frame| {
                    let theme = Theme::current();
                    frame.buffer_mut().set_style(
                        area,
                        Style::default().fg(theme.text_primary).bg(theme.bg_base),
                    );
                    let mut prompt =
                        ScrollbackEntry::new(RenderBlock::user_prompt("Inspect the renderer"));
                    prompt.created_at = None;
                    EntryRenderer::new(&prompt, &theme)
                        .render(Rect::new(2, 2, width - 4, 3), frame.buffer_mut());
                    let renderer = EntryRenderer::new(&entry, &theme)
                        .with_groupable(true)
                        .with_selected(state.ends_with("selected"))
                        .with_tick(if state == "running-tick" { 10 } else { 0 });
                    let h = renderer.desired_height(width - 4).min(height - 6);
                    renderer.render(Rect::new(2, 6, width - 4, h), frame.buffer_mut());
                })?;
                drop(terminal);
                fs::write(
                    directory.join(format!(
                        "align-{family}-{state}-{width}x{height}-motion-0ms.ansi"
                    )),
                    bytes,
                )?;
            }
        }
    }
    fs::write(
        directory.join("producer.json"),
        r#"{"entrypoints":["xai_grok_pager::scrollback::wrappers::EntryRenderer::render"],"scope":"Native tool block families plus task, todo, MCP and question; source-defined layout, no Harness chrome","timing":{"mode":"injected entry tick 0 or 10; stationary geometry across lifecycle states"}}"#,
    )?;
    Ok(())
}

fn block(family: &str, state: &str) -> RenderBlock {
    let failed = state.starts_with("failed");
    let done = failed || state.starts_with("succeeded");
    if family == "task" {
        let task = if failed {
            SubagentBlock::failed(
                "Edit temporary tool test file",
                "child",
                Duration::ZERO,
                Some("Invalid fixture selector".into()),
            )
        } else if done {
            SubagentBlock::completed("Edit temporary tool test file", "child", Duration::ZERO)
        } else {
            SubagentBlock::started(
                "Edit temporary tool test file",
                "child",
                "general",
                None,
                None,
                None,
                false,
            )
        };
        return RenderBlock::Subagent(task);
    }
    let tool = match family {
        "read" => {
            let mut tool = ReadToolCallBlock::new("demo.txt");
            if done && !failed {
                tool = tool.with_content("ready".into(), 1);
            }
            if failed {
                tool = tool.with_error("Invalid fixture selector");
            }
            ToolCallBlock::Read(tool)
        }
        "write" | "edit" | "apply-patch" => {
            let before = if family == "edit" { "old\n" } else { "" };
            let hunks = if done && !failed {
                xai_grok_pager_diff::diff_hunks_from_strings(
                    before,
                    "Initial tool test content.\n",
                    1,
                )
            } else {
                vec![]
            };
            let mut tool = EditToolCallBlock::new("demo.txt", hunks);
            if failed {
                tool = tool.with_error("Invalid fixture selector");
            }
            ToolCallBlock::Edit(tool)
        }
        "bash" => {
            let mut tool = ExecuteToolCallBlock::new("printf ready");
            tool.description = Some("Check terminal output".into());
            if done && !failed {
                tool = tool.with_output("ready");
            }
            if failed {
                tool = tool.with_error("Invalid fixture selector");
            }
            ToolCallBlock::Execute(tool)
        }
        "grep" | "glob" => {
            let mut tool = SearchToolCallBlock::new("ready");
            if family == "glob" {
                tool.meta.output_mode = SearchOutputMode::FilesWithMatches;
            }
            if done && !failed {
                if family == "glob" {
                    tool.file_paths = vec!["demo.txt".into()];
                    tool.match_count = 1;
                } else {
                    tool = tool.with_matches(
                        1,
                        vec![SearchFileMatch {
                            path: "demo.txt".into(),
                            matches: vec![SearchLineMatch {
                                line_number: 1,
                                content: "ready".into(),
                            }],
                        }],
                    );
                }
            }
            ToolCallBlock::Search(tool)
        }
        "websearch" => {
            let mut tool = WebSearchToolCallBlock::new("terminal geometry");
            if done && !failed {
                tool.content = Some("ready".into());
            }
            ToolCallBlock::WebSearch(tool)
        }
        "list" => ToolCallBlock::ListDir(ListDirToolCallBlock::new("src").with_output("ready")),
        _ => {
            let mut tool = OtherToolCallBlock::new(family, "");
            if done && !failed {
                let output = if family == "question" {
                    "User has answered your questions: \"Choose an option\"=\"First\""
                } else {
                    "ready"
                };
                tool = tool.with_output(output);
            }
            if failed {
                tool.set_error(Some("Invalid fixture selector".into()));
            }
            ToolCallBlock::Other(tool)
        }
    };
    RenderBlock::ToolCall(tool)
}

//! Capture production entry painting, including bullet/rail animation, with synthetic data.
use std::{fs, path::Path};
use ratatui::{backend::CrosstermBackend, layout::Rect, style::Style, Terminal, TerminalOptions, Viewport};
use xai_grok_pager::{
    render::Renderable,
    scrollback::{block::RenderBlock, entry::ScrollbackEntry, types::DisplayMode, wrappers::EntryRenderer},
    theme::{Theme, ThemeKind},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args().nth(1).ok_or("output directory required")?;
    let directory = Path::new(&directory);
    fs::create_dir_all(directory)?;
    Theme::apply_kind(ThemeKind::GrokNight);
    for (width, height) in [(40,24), (80,24), (120,40)] {
        for reduced in [false, true] {
            for scene in ["thinking", "thinkingcode", "running", "runningopen", "success", "successopen", "failed", "failedopen", "answer", "context", "contextopen", "searchsources", "commands", "commandsopen"] {
                let block = match scene {
                    "thinking" => RenderBlock::thinking("Check **terminal geometry**, then compare the tool output.\n\nKeep the title stationary while the diamond and rail animate."),
                    "thinkingcode" => RenderBlock::thinking("```rust\nlet text = r#\"\ninside the string\n\"#;"),
                    "answer" => RenderBlock::agent_message("The **renderer** preserves Unicode: 界 é 👩‍💻.\n\n```rust\nfn main() {\n    println!(\"ready\");\n}\n```\n\n- Stable text\n- Safe output"),
                    _ => {
                        let mut tool = xai_grok_pager::scrollback::blocks::ExecuteToolCallBlock::new("printf 'ready\\n'");
                        tool.description = Some("Check terminal output".into());
                        if scene.starts_with("success") { tool = tool.with_output("ready"); }
                        if scene.starts_with("failed") { tool = tool.with_error("terminal unavailable"); }
                        RenderBlock::ToolCall(xai_grok_pager::scrollback::blocks::ToolCallBlock::Execute(tool))
                    }
                };
                let mut entry = ScrollbackEntry::new(block);
                entry.created_at = None;
                entry.is_running = scene.starts_with("thinking") || scene.starts_with("running");
                entry.display_mode = if scene.starts_with("thinking") { DisplayMode::Truncated } else if scene == "answer" || scene.ends_with("open") { DisplayMode::Expanded } else { DisplayMode::Collapsed };
                for milliseconds in [0,330,660] {
                    let mut bytes = Vec::new();
                    let area = Rect::new(0,0,width,height);
                    let mut terminal = Terminal::with_options(CrosstermBackend::new(&mut bytes), TerminalOptions { viewport: Viewport::Fixed(area) })?;
                    terminal.draw(|frame| {
                        let theme = Theme::current();
                        frame.buffer_mut().set_style(area, Style::default().fg(theme.text_primary).bg(theme.bg_base));
                        let mut prompt = ScrollbackEntry::new(RenderBlock::user_prompt("Inspect the renderer"));
                        prompt.created_at = None;
                        let prompt_renderer = EntryRenderer::new(&prompt, &theme);
                        let prompt_height = prompt_renderer.desired_height(width);
                        prompt_renderer.render(Rect::new(0,0,width,prompt_height), frame.buffer_mut());
                        let remaining = Rect::new(0, prompt_height+1, width, height-prompt_height-1);
                        if scene.starts_with("context") || scene.starts_with("commands") || scene == "searchsources" {
                            render_group(scene, remaining, frame.buffer_mut(), if reduced {0} else {milliseconds/33});
                        } else {
                            let renderer = EntryRenderer::new(&entry,&theme).with_groupable(scene != "answer" && !scene.starts_with("thinking")).with_tick(if reduced {0} else {milliseconds/33});
                            let entry_height = renderer.desired_height(width);
                            renderer.render(Rect::new(remaining.x,remaining.y,width,entry_height.min(remaining.height)), frame.buffer_mut());
                        }
                    })?;
                    drop(terminal);
                    fs::write(directory.join(format!("chat-{scene}-{width}x{height}-{}-{milliseconds}ms.ansi", if reduced {"reduced"} else {"motion"})),bytes)?;
                }
            }
        }
    }
    fs::write(directory.join("producer.json"), r#"{"entrypoints":["xai_grok_pager::scrollback::wrappers::EntryRenderer::render","xai_grok_pager::scrollback::ScrollbackState::prepare_layout","xai_grok_pager::scrollback::ScrollbackPane::render_with_scratch"],"timing":{"mode":"injected reference animation ticks (33 ms); reduced fixtures hold tick zero"},"scope":"Production scrollback entries and group state; Harness shell chrome excluded"}"#)?;
    Ok(())
}

fn render_group(scene: &str, area: Rect, buffer: &mut ratatui::buffer::Buffer, tick: u64) {
    use xai_grok_pager::scrollback::{ScrollbackState, ScrollbackPane, ScratchBuffer};
    use xai_grok_pager::scrollback::blocks::tool::{ReadToolCallBlock, ExecuteToolCallBlock, WebSearchToolCallBlock, ToolCallBlock};
    let mut state = ScrollbackState::new();
    if scene.starts_with("context") {
        let mut thought = RenderBlock::thinking("Inspect the files first.");
        if let RenderBlock::Thinking(block) = &mut thought { block.set_elapsed_time_ms(Some(2)); }
        state.push(ScrollbackEntry::new(thought).with_display_mode(DisplayMode::Collapsed));
    }
    for index in 0..if scene.starts_with("commands") {14} else {2} {
        let tool = if scene.starts_with("commands") {
            ToolCallBlock::Execute(ExecuteToolCallBlock::new(format!("printf command-{index:02}")).with_output("recorded output"))
        } else if scene == "searchsources" {
            let mut tool = WebSearchToolCallBlock::new(format!("query {index}"));
            tool.content = Some("recorded output".into());
            tool.citations = vec!["https://example.com/shared".into(), format!("https://example.com/{index}")];
            ToolCallBlock::WebSearch(tool)
        } else {
            ToolCallBlock::Read(ReadToolCallBlock::new(format!("src/file-{index}.rs")).with_content("recorded output".into(), 1))
        };
        state.push(ScrollbackEntry::new(RenderBlock::ToolCall(tool)).with_display_mode(DisplayMode::Collapsed));
    }
    if scene.starts_with("context") {
        let mut thought = ScrollbackEntry::new(RenderBlock::thinking("Still checking **sources**."));
        thought.is_running = true;
        thought.display_mode = DisplayMode::Truncated;
        state.push(thought);
    }
    state.prepare_layout(area.width, area.height);
    if scene.ends_with("open") {
        state.set_selected(Some(0));
        assert!(state.toggle_group_expansion());
        state.clear_selection();
        state.prepare_layout(area.width, area.height);
    }
    for _ in 0..tick { state.tick(); }
    ScrollbackPane::new().render_with_scratch(area, buffer, &state, &mut ScratchBuffer::new());
}

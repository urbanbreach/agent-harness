//! Reference-only evidence driver: public production renderers, fixture data, no provider requests.
use std::{fs, path::Path, sync::Arc};
use ratatui::{backend::CrosstermBackend, layout::Rect, style::Style, text::Line, widgets::Paragraph, Terminal, TerminalOptions, Viewport};
use xai_grok_pager::{
    actions::ActionRegistry,
    app::roster::{RosterActivity, RosterEntry, RosterOrigin},
    scrollback::{block::BlockContent, blocks::{markdown_content::MarkdownContent, ReadToolCallBlock, ExecuteToolCallBlock}, types::{BlockContext, DisplayMode}},
    settings::{SettingsRegistry, PagerLocalSnapshot},
    theme::{Theme, ThemeKind},
    views::{dashboard::{render_dashboard, DashboardState}, settings_modal::{render_settings_modal, SettingsModalState}},
};
const MARKDOWN: &str = "**outer *inner* end** and **[reference](https://example.com)**.\n\n~~~rust\nfn main() {\n\tlet value = 42;\n    println!(\"{value}\");\n}\n~~~\n\n| Left | Center | Right |\n| :--- | :---: | ---: |\n| alpha | beta | 123 |\n\n> outer quote\n> > nested quote with wrapped words\n\n$E=mc^2$\n\n";
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).ok_or("output directory required")?;
    fs::create_dir_all(&output)?;
    for (profile,kind) in [("dark",ThemeKind::GrokNight),("light",ThemeKind::GrokDay)] {
        Theme::apply_kind(kind);
        for (width,height) in [(24,24),(40,40),(80,24),(120,40),(160,50)] {
            for scene in ["markdown", "streaming", "tools", "dashboard", "settings"] {
                let area = Rect::new(0,0,width,height);
                let mut bytes = Vec::new();
                let mut terminal = Terminal::with_options(CrosstermBackend::new(&mut bytes), TerminalOptions {viewport:Viewport::Fixed(area)})?;
                terminal.draw(|frame| {
                    let theme=Theme::current();
                    frame.buffer_mut().set_style(area, Style::default().fg(theme.text_primary).bg(theme.bg_base));
                    match scene {
                        "markdown" | "streaming" => {
                            let md = if scene == "streaming" { let mut md=MarkdownContent::streaming(); md.push_chunk(MARKDOWN); md } else {MarkdownContent::new(MARKDOWN)};
                            let lines=md.output(usize::from(width)).lines.into_iter().map(|row| row.content).collect::<Vec<_>>();
                            frame.render_widget(Paragraph::new(lines),area);
                        }
                        "tools" => {
                            let context=BlockContext {mode:DisplayMode::Expanded,is_running:false,width,raw:false,max_lines:None,appearance:Default::default(),is_selected:true,cwd:None};
                            let read=ReadToolCallBlock::new("src/ui.rs").with_content("/* recorded multiline\n   comment */\nfn main() {\n\tlet value = 42;\n}\n".into(),5);
                            let shell=ExecuteToolCallBlock::new("cargo test -p harness-tui").with_output("\u{1b}[31mred\u{1b}[0m\nprogress 1\rprogress 2\u{1b}[K");
                            let mut lines=read.output(&context).lines.into_iter().map(|row|row.content).collect::<Vec<_>>();
                            lines.push(Line::default());
                            lines.extend(shell.output(&context).lines.into_iter().map(|row|row.content));
                            frame.render_widget(Paragraph::new(lines),area);
                        }
                        "dashboard" => {
                            let roster=[("awaiting-parent",RosterActivity::NeedsInput),("working-parser",RosterActivity::Working),("working-terminal",RosterActivity::Working)].map(|(name,activity)|RosterEntry {session_id:name.into(),title:Some(name.into()),cwd:"/workspace/agent-harness".into(),is_worktree:false,model_id:None,yolo:false,activity,last_turn_summary:None,resident:true,last_change_unix_ms:1_725_000_000_000,origin:RosterOrigin::default()});
                            render_dashboard(frame.buffer_mut(),area,&mut DashboardState::new(),&mut Default::default(),&ActionRegistry::defaults(),None,&roster,false,None,false,None);
                        }
                        "settings" => {
                            let mut state=SettingsModalState::new(Arc::new(SettingsRegistry::defaults()),Default::default(),PagerLocalSnapshot::default());
                            render_settings_modal(frame.buffer_mut(),area,&mut state,false,None);
                        }
                        _ => {}
                    }
                })?;
                drop(terminal);
                fs::write(Path::new(&output).join(format!("{scene}-{profile}-{width}x{height}-reduced-0ms.ansi")),bytes)?;
            }
        }
    }
    fs::write(Path::new(&output).join("runtime-timing.json"),"[]")?;
    Ok(())
}

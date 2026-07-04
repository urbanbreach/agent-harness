use super::*;

use super::opencode_subagent_parity_apps as app_fixtures;
use crate::ui::render_app;
use ratatui::style::{Color, Modifier};
use ratatui::{backend::TestBackend, Terminal};

const EVIDENCE_ENV: &str = "HARNESS_TUI_OPENCODE_SUBAGENT_EVIDENCE_DIR";

pub(super) fn opencode_subagent_parity_evidence_export() {
    let root = std::env::var_os(EVIDENCE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!("set {EVIDENCE_ENV} before running this ignored evidence export")
        });

    fs::create_dir_all(root.join("dogfood")).expect("create dogfood evidence dir");
    let mut manifest = Vec::new();

    let no_child = app_fixtures::no_child_app();
    export_state(&root, &mut manifest, "no-child", &no_child);

    let running = app_fixtures::foreground_running_app();
    export_state(&root, &mut manifest, "foreground-running", &running);

    let background_affordance = app_fixtures::background_affordance_app();
    export_state(
        &root,
        &mut manifest,
        "background-affordance",
        &background_affordance,
    );

    let completed = app_fixtures::completed_app();
    export_state(&root, &mut manifest, "completed", &completed);

    let retry = app_fixtures::retry_app();
    export_state(&root, &mut manifest, "retry-error", &retry);

    let background = app_fixtures::background_completed_app();
    export_state(&root, &mut manifest, "background", &background);

    let footer = app_fixtures::child_footer_app();
    export_state(&root, &mut manifest, "footer", &footer);

    for target in [
        SubagentFooterTarget::Parent,
        SubagentFooterTarget::Previous,
        SubagentFooterTarget::Next,
    ] {
        let mut hovered = app_fixtures::child_footer_app();
        hovered.hovered_subagent_footer_target = Some(target);
        export_state(
            &root,
            &mut manifest,
            subagent_footer_hover_slug(target),
            &hovered,
        );
    }

    let siblings = app_fixtures::sibling_after_navigation_app();
    export_state(&root, &mut manifest, "siblings", &siblings);

    fs::write(
        root.join("dogfood/manifest.txt"),
        manifest.join("\n") + "\n",
    )
    .expect("write dogfood manifest");
}

fn export_state(root: &Path, manifest: &mut Vec<String>, slug: &str, app: &AppState) {
    for width in [80_u16, 120, 160] {
        let file = format!("dogfood/{slug}-{width}.txt");
        let rendered = render_text(app, width, 40);
        fs::write(root.join(&file), rendered).expect("write dogfood capture");
        manifest.push(file);

        let ansi_file = format!("dogfood/{slug}-{width}.ansi.txt");
        fs::write(root.join(&ansi_file), render_ansi(app, width, 40))
            .expect("write dogfood ANSI capture");
        manifest.push(ansi_file);
    }
}

fn subagent_footer_hover_slug(target: SubagentFooterTarget) -> &'static str {
    match target {
        SubagentFooterTarget::Parent => "footer-hover-parent",
        SubagentFooterTarget::Previous => "footer-hover-prev",
        SubagentFooterTarget::Next => "footer-hover-next",
    }
}

fn render_ansi(app: &AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| render_app(frame, app))
        .expect("draw frame");

    let mut rendered = String::new();
    for row in terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(width))
    {
        for cell in row {
            rendered.push_str(&ansi_style(cell.fg, cell.bg, cell.modifier));
            rendered.push_str(cell.symbol());
        }
        rendered.push_str("\x1b[0m\n");
    }
    rendered
}

fn ansi_style(fg: Color, bg: Color, modifier: Modifier) -> String {
    let mut codes = vec!["0".to_string()];
    if modifier.contains(Modifier::BOLD) {
        codes.push("1".to_string());
    }
    if modifier.contains(Modifier::UNDERLINED) {
        codes.push("4".to_string());
    }
    append_color_code(&mut codes, fg, AnsiColorRole::Foreground);
    append_color_code(&mut codes, bg, AnsiColorRole::Background);
    format!("\x1b[{}m", codes.join(";"))
}

#[derive(Clone, Copy)]
enum AnsiColorRole {
    Foreground,
    Background,
}

impl AnsiColorRole {
    const fn base(self) -> u8 {
        match self {
            Self::Foreground => 30,
            Self::Background => 40,
        }
    }

    const fn extended_prefix(self) -> u8 {
        match self {
            Self::Foreground => 38,
            Self::Background => 48,
        }
    }
}

fn append_color_code(codes: &mut Vec<String>, color: Color, role: AnsiColorRole) {
    let base = role.base();
    match color {
        Color::Reset => {}
        Color::Black => codes.push(base.to_string()),
        Color::Red => codes.push((base + 1).to_string()),
        Color::Green => codes.push((base + 2).to_string()),
        Color::Yellow => codes.push((base + 3).to_string()),
        Color::Blue => codes.push((base + 4).to_string()),
        Color::Magenta => codes.push((base + 5).to_string()),
        Color::Cyan => codes.push((base + 6).to_string()),
        Color::Gray => codes.push((base + 7).to_string()),
        Color::DarkGray => codes.push((base + 60).to_string()),
        Color::LightRed => codes.push((base + 61).to_string()),
        Color::LightGreen => codes.push((base + 62).to_string()),
        Color::LightYellow => codes.push((base + 63).to_string()),
        Color::LightBlue => codes.push((base + 64).to_string()),
        Color::LightMagenta => codes.push((base + 65).to_string()),
        Color::LightCyan => codes.push((base + 66).to_string()),
        Color::White => codes.push((base + 67).to_string()),
        Color::Rgb(red, green, blue) => {
            codes.push(format!("{};2;{red};{green};{blue}", role.extended_prefix()));
        }
        Color::Indexed(index) => {
            codes.push(format!("{};5;{index}", role.extended_prefix()));
        }
    }
}

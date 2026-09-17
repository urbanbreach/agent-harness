use super::*;
use crate::app::SettingsTab;

pub(super) const CLOSE_TARGET_WIDTH: u16 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalTabs {
    pub(crate) labels: [&'static str; 2],
    pub(crate) selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalChrome {
    pub(crate) title: &'static str,
    pub(crate) breadcrumb: Option<&'static str>,
    pub(crate) tabs: Option<ModalTabs>,
    pub(crate) footer: &'static str,
}

pub(super) const COMMANDS_CHROME: ModalChrome = ModalChrome {
    title: "Commands",
    breadcrumb: None,
    tabs: None,
    footer: "↑/↓ nav · Enter select · Esc close",
};

pub(super) const MODELS_CHROME: ModalChrome = ModalChrome {
    title: "Models",
    breadcrumb: None,
    tabs: None,
    footer: "↑/↓ navigate · Enter select · Esc close",
};

pub(crate) const HELP_CHROME: ModalChrome = ModalChrome {
    title: "Keyboard Shortcuts",
    breadcrumb: None,
    tabs: None,
    footer: "Enter details · / search · Esc close",
};

pub(super) const fn settings_chrome(tab: SettingsTab) -> ModalChrome {
    ModalChrome {
        title: "Settings",
        breadcrumb: Some("Commands / Settings"),
        tabs: Some(ModalTabs {
            labels: ["Runtime", "TUI"],
            selected: match tab {
                SettingsTab::Runtime => 0,
                SettingsTab::Tui => 1,
            },
        }),
        footer: "↑/↓ navigate · Enter edit · Esc close",
    }
}

pub(super) fn centered_popup(
    root: Rect,
    min_width: u16,
    max_width: u16,
    min_height: u16,
    max_height: u16,
) -> Rect {
    let width = root.width.clamp(min_width, max_width).min(root.width);
    let height = root.height.clamp(min_height, max_height).min(root.height);
    Rect::new(
        root.x + root.width.saturating_sub(width) / 2,
        root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

pub(super) fn close_area(popup: Rect) -> Rect {
    let width = CLOSE_TARGET_WIDTH.min(popup.width);
    Rect::new(popup.right().saturating_sub(width), popup.y, width, 1)
}

pub(super) fn footer_area(popup: Rect) -> Option<Rect> {
    (popup.width > 4 && popup.height > 3).then_some(Rect::new(
        popup.x.saturating_add(2),
        popup.bottom().saturating_sub(2),
        popup.width.saturating_sub(4),
        1,
    ))
}

pub(super) fn render_body(frame: &mut Frame, theme: &Theme, popup: Rect, chrome: ModalChrome) {
    let surface = ui_chrome::command_palette_surface(theme);
    let primary = Style::default()
        .fg(ui_chrome::command_palette_title(theme))
        .bg(surface);
    let muted = Style::default()
        .fg(ui_chrome::command_palette_muted(theme))
        .bg(surface);

    if let Some(breadcrumb) = chrome.breadcrumb.filter(|_| popup.height > 3) {
        frame.render_widget(
            Paragraph::new(Span::styled(
                truncate_plain_text(breadcrumb, usize::from(popup.width.saturating_sub(4))),
                muted,
            )),
            Rect::new(
                popup.x.saturating_add(2),
                popup.y.saturating_add(1),
                popup.width.saturating_sub(4),
                1,
            ),
        );
    }
    if let Some(tabs) = chrome.tabs.filter(|_| popup.height > 4) {
        let text = tabs
            .labels
            .iter()
            .enumerate()
            .flat_map(|(index, label)| {
                let selected = index == tabs.selected;
                [
                    Span::styled(
                        if selected {
                            format!("[{label}]")
                        } else {
                            (*label).to_string()
                        },
                        if selected {
                            primary.fg(theme.text.accent).add_modifier(Modifier::BOLD)
                        } else {
                            muted
                        },
                    ),
                    Span::raw("  "),
                ]
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Line::from(text)),
            Rect::new(
                popup.x.saturating_add(2),
                popup.y.saturating_add(2),
                popup.width.saturating_sub(4),
                1,
            ),
        );
    }
    if let Some(area) = footer_area(popup) {
        frame.render_widget(
            Paragraph::new(Span::styled(chrome.footer, muted)).alignment(Alignment::Center),
            area,
        );
    }
}

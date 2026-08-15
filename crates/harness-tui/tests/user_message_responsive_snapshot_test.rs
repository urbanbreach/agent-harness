use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, Focus, Tab};
use harness_tui::render_test::{buffer_to_string, render_to_buffer};
use harness_tui::ui;
use ratatui::layout::Rect;

const VIEWPORTS: [(u16, u16); 6] = [
    (60, 20),
    (79, 24),
    (80, 24),
    (100, 30),
    (120, 40),
    (132, 40),
];

const WRAPPED_PROMPT: &str = "Explain how responsive transcript messages preserve readable wrapping while the available width changes.";
const MULTILINE_PROMPT: &str = "Check the renderer.\nKeep this hard line break.\n验证 CJK 宽度。";
const LONG_PROMPT: &str = concat!(
    "This deliberately long prompt must collapse after three visual rows while preserving grapheme boundaries, CJK width correctness, timestamps, selection geometry, copy behavior, mouse targeting, and replay-safe text projection. ",
    "This deliberately long prompt must collapse after three visual rows while preserving grapheme boundaries, CJK width correctness, timestamps, selection geometry, copy behavior, mouse targeting, and replay-safe text projection. ",
    "This deliberately long prompt must collapse after three visual rows while preserving grapheme boundaries, CJK width correctness, timestamps, selection geometry, copy behavior, mouse targeting, and replay-safe text projection. ",
    "This deliberately long prompt must collapse after three visual rows while preserving grapheme boundaries, CJK width correctness, timestamps, selection geometry, copy behavior, mouse targeting, and replay-safe text projection. ",
    "This deliberately long prompt must collapse after three visual rows while preserving grapheme boundaries, CJK width correctness, timestamps, selection geometry, copy behavior, mouse targeting, and replay-safe text projection."
);

fn user_message_app(text: &str, selected: bool) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    if selected {
        app.set_selected_activity_index_for_test(0);
        app.focus = Focus::Details;
    }
    app.ingest_event(EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: "evt-user-message-responsive".to_string(),
        seq: 1,
        run_id: "run-user-message-responsive".into(),
        mono_ms: 1,
        ts: Some("2026-08-14T12:34:56Z".to_string()),
        actor: EventActor::new(ActorKind::User, None),
        correlation_id: Some("req-user-message-responsive".to_string()),
        causation_id: None,
        stream_key: Some("run:run-user-message-responsive".to_string()),
        payload: EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req-user-message-responsive".into(),
            text: text.to_string(),
        }),
    });
    app
}

fn render(app: &AppState, width: u16, height: u16) -> ratatui::buffer::Buffer {
    render_to_buffer(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn trim_trailing_whitespace(rendered: &str) -> String {
    rendered
        .lines()
        .map(|line| {
            line.find('').map_or_else(
                || line.trim_end().to_string(),
                |branch_column| format!("{} [workspace]", &line[..branch_column]),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

fn viewport_snapshot(width: u16, height: u16) -> String {
    let states = [
        ("one line", "Ship the elevated user-message band."),
        ("wrapped", WRAPPED_PROMPT),
        ("multiline", MULTILINE_PROMPT),
        ("selected", "This prompt is selected."),
        ("long collapsed", LONG_PROMPT),
    ];
    let mut sections = Vec::with_capacity(states.len());
    for (label, prompt) in states {
        let app = user_message_app(prompt, label == "selected");
        if label == "selected" {
            assert_eq!(app.focus, Focus::Details);
            assert_eq!(app.active_tab, Tab::Run);
            assert!(!app.details_drawer_open());
            assert_eq!(
                app.transcript_interaction_snapshot()
                    .selected_activity_index,
                0
            );
        }
        let buffer = render(&app, width, height);
        let rendered = buffer_to_string(&buffer, width);
        let prompt_row = rendered
            .lines()
            .position(|line| line.contains(prompt.split_whitespace().next().unwrap_or_default()));
        assert!(prompt_row.is_some(), "rendered user prompt row");
        let prompt_row = prompt_row.unwrap_or_default();
        let gutter = usize::from(app.theme().live_shell.rhythm.transcript_gutter_x);
        let expected_surface = if label == "selected" {
            app.theme().surface.selected_card
        } else {
            app.theme().surface.card
        };
        let row_has_expected_surface = |row_index: usize| {
            let row_start = row_index * usize::from(width);
            buffer.content[row_start + gutter..row_start + usize::from(width) - gutter]
                .iter()
                .all(|cell| cell.bg == expected_surface)
        };
        assert!(
            row_has_expected_surface(prompt_row),
            "{width}x{height} {label}: user content row must fill the semantic card surface"
        );
        let surface_start = (0..=prompt_row)
            .rev()
            .take_while(|row| row_has_expected_surface(*row))
            .last()
            .unwrap_or(prompt_row);
        let bottom_padding_row = (prompt_row + 1..usize::from(height)).find(|row| {
            row_has_expected_surface(*row)
                && rendered
                    .lines()
                    .nth(*row)
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
        });
        assert!(
            surface_start < prompt_row && bottom_padding_row.is_some(),
            "{width}x{height} {label}: user band must retain blank top and bottom padding"
        );
        assert!(
            rendered
                .lines()
                .nth(surface_start)
                .unwrap_or_default()
                .trim()
                .is_empty(),
            "{width}x{height} {label}: top padding row must be visually blank"
        );
        assert!(
            rendered
                .lines()
                .nth(bottom_padding_row.unwrap_or(prompt_row))
                .unwrap_or_default()
                .trim()
                .is_empty(),
            "{width}x{height} {label}: bottom padding row must be visually blank"
        );
        let visible_prompt = rendered
            .lines()
            .nth(prompt_row)
            .unwrap_or_default()
            .trim_start();
        assert!(
            visible_prompt.starts_with("› "),
            "{width}x{height} {label}: every user message must use the compact Grok marker"
        );
        assert!(
            !visible_prompt.starts_with("You  "),
            "{width}x{height} {label}: the rejected width-dependent label must not return"
        );
        sections.push(format!(
            "--- {label} ---\n{}",
            trim_trailing_whitespace(&rendered)
        ));
    }
    sections.join("\n\n")
}

#[test]
fn user_messages_match_responsive_anatomy_at_measured_viewports() {
    for (width, height) in VIEWPORTS {
        insta::assert_snapshot!(
            format!("responsive_user_messages_{width}x{height}"),
            viewport_snapshot(width, height)
        );
    }
}

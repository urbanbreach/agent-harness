#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast assertions for deterministic dashboard fixtures"
)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use harness_tui::app::Focus;
use harness_tui::dashboard::{
    DashboardActivity, DashboardEntryEligibility, DashboardGroup, DashboardGroupKey,
    DashboardReadModel, DashboardRelationship, DashboardRow, DashboardStatus, SelectionKey,
};
use harness_tui::dashboard_controls::DashboardControlState;
use harness_tui::dashboard_integration::{
    DashboardBreakpoint, DashboardFocus, DashboardInput, DashboardInputRouter,
    DashboardIntegration, DashboardIntegrationParts, DashboardModal, DashboardModalKind,
    DashboardMouseContext, DashboardOverlayRoute, DashboardOverlayState, DashboardPane,
    DashboardReturnState, FocusDirection, SearchContext, layout_for_rect,
};
use harness_tui::overlay::OverlayKind;
use harness_tui::shell_geometry::ShellState;
use harness_tui::transcript_identity::TranscriptFocus;
use ratatui::layout::Rect;

fn model() -> DashboardReadModel {
    let rows = [
        row("alpha", "Alpha session", DashboardStatus::Running),
        row("beta", "Beta session", DashboardStatus::Streaming),
    ]
    .into_iter()
    .collect::<Vec<_>>();
    DashboardReadModel {
        groups: rows
            .iter()
            .map(|item| DashboardGroup {
                key: item.relationship.group.clone(),
                row_keys: vec![item.selection_key.clone()],
            })
            .collect(),
        all_rows: rows.clone(),
        rows,
    }
}

fn row(id: &str, title: &str, status: DashboardStatus) -> DashboardRow {
    let key = SelectionKey::new(id);
    DashboardRow {
        selection_key: key.clone(),
        title: Some(title.to_string()),
        status,
        activity: DashboardActivity {
            last_event_seq: 1,
            last_event_id: Some(format!("event-{id}")),
            unread_count: 0,
        },
        relationship: DashboardRelationship {
            parent: None,
            children: Vec::new(),
            group: DashboardGroupKey::Root(key),
            lineage_depth: 0,
            parent_missing: false,
            is_parent: false,
            is_child: false,
            is_background: false,
            is_foreign: false,
        },
        eligibility: DashboardEntryEligibility {
            is_eligible: true,
            excluded_by: None,
        },
        creation_seq: 1,
    }
}

fn integration() -> DashboardIntegration {
    let dashboard = model();
    let selected = SelectionKey::new("alpha");
    DashboardIntegration::new(
        DashboardIntegrationParts {
            controls: DashboardControlState::new(
                dashboard.clone(),
                Some(selected.clone()),
                "draft survives",
            ),
            dashboard,
            peek: harness_tui::dashboard_peek::DashboardPeek::new(8.0)
                .expect("valid peek viewport"),
            roster: harness_tui::dashboard_roster::RosterState {
                selected: Some(selected),
                ..Default::default()
            },
            details: None,
        },
        Rect::new(0, 0, 132, 40),
    )
    .expect("valid dashboard integration")
}

#[test]
fn focus_tab_and_shift_tab_cycle_without_losing_focus() {
    let mut focus = DashboardFocus::new(DashboardPane::Roster);
    let visible = DashboardPane::ALL;

    for expected in [
        DashboardPane::Peek,
        DashboardPane::Reply,
        DashboardPane::Details,
        DashboardPane::Roster,
    ] {
        focus.traverse(FocusDirection::Forward, &visible);
        assert_eq!(focus.current(), expected);
    }
    focus.traverse(FocusDirection::Backward, &visible);
    assert_eq!(focus.current(), DashboardPane::Details);
}

#[test]
fn modal_overlay_precedes_dashboard_chrome_and_pane_targets() {
    let mut overlays = DashboardOverlayState::new();
    overlays.open_chrome(OverlayKind::DetailsDrawer);
    assert_eq!(
        overlays.route(DashboardPane::Roster),
        DashboardOverlayRoute::Chrome(OverlayKind::DetailsDrawer)
    );

    overlays.open_modal(DashboardModal::Permission("permission-1".to_string()));
    assert_eq!(
        overlays.route(DashboardPane::Roster),
        DashboardOverlayRoute::Modal(DashboardModalKind::Permission)
    );
}

#[test]
fn responsive_layout_uses_shell_geometry_and_hides_details_at_narrow_widths() {
    let compact = layout_for_rect(Rect::new(0, 0, 60, 15), ShellState::Streaming);
    let wide = layout_for_rect(Rect::new(0, 0, 132, 40), ShellState::Streaming);

    assert_eq!(compact.breakpoint, DashboardBreakpoint::Compact);
    assert!(!compact.visibility.details);
    assert!(compact.shell.contains_all_regions());
    assert_eq!(wide.breakpoint, DashboardBreakpoint::Wide);
    assert!(wide.visibility.details);
    assert!(wide.visibility.roster && wide.visibility.peek && wide.visibility.reply);
}

#[test]
fn keyboard_and_roster_mouse_routes_reach_dashboard_targets() {
    let mut dashboard = integration();
    let router = DashboardInputRouter::new();
    assert_eq!(
        router.route_key(
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            DashboardPane::Roster,
            &DashboardOverlayState::new(),
        ),
        DashboardInput::Focus(FocusDirection::Forward)
    );
    let layout = dashboard.roster_layout();
    let row = layout.rows.first().expect("roster row");
    let hit_map = dashboard.roster_hit_map();
    let overlays = DashboardOverlayState::new();
    let input = router.route_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: row.rect.x,
            row: row.rect.y,
            modifiers: KeyModifiers::NONE,
        },
        DashboardMouseContext {
            roster: &hit_map,
            layout: dashboard.layout(),
            overlays: &overlays,
        },
    );
    assert_eq!(input, DashboardInput::Select(row.selection_key.clone()));
    dashboard.handle(input).expect("selection route");
    assert_eq!(dashboard.focus(), DashboardPane::Roster);
}

#[test]
fn search_help_hooks_and_return_state_are_contextual_and_exact() {
    let mut dashboard = integration();
    dashboard.begin_search(SearchContext::Roster);
    dashboard.input_search("Alpha").expect("search input");
    assert_eq!(dashboard.roster_state().filter.query, "Alpha");
    let help = dashboard.help(Focus::List);
    assert!(help.entries.iter().any(|entry| entry.action == "search"));

    let prior = DashboardReturnState::new(TranscriptFocus::Timeline, true, None);
    dashboard.capture_return_state(prior.clone());
    dashboard.notify_task_completed("alpha");
    assert!(dashboard.title().contains("alpha"));
    assert_eq!(dashboard.return_state(), Some(&prior));
    assert_eq!(dashboard.leave(), prior);
}

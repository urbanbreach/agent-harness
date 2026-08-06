use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_tui::dashboard::{
    DashboardEligibilityRules, DashboardReplayRegistry, DashboardSessionInput, SelectionKey,
};
use harness_tui::dashboard_details::{
    CycleDirection, DashboardDetails, DetailsLayoutMode, NavigationError, RosterState,
};
use harness_tui::UnwrapOrAbort;
use ratatui::layout::Rect;

fn catalog(id: &str, parent: Option<&str>) -> SessionCatalogEntry {
    SessionCatalogEntry {
        run_id: id.to_string(),
        run_name: Some(format!("Session {id}")),
        status: Some(RunStatus::Running),
        last_updated_at: Some("2026-08-04T00:00:00Z".to_string()),
        workspace_root: Some("/workspace".to_string()),
        profile_preset: Some("build".to_string()),
        provider_model: Some("mock/model".to_string()),
        mode_source: SessionModeSource::InteractiveLive,
        is_resumable: true,
        resume_disabled_reason: None,
        artifact_count: 2,
        child_session_count: 0,
        parent_session_id: parent.map(str::to_string),
    }
}

fn registry(include_grandchild: bool, include_sibling: bool) -> DashboardReplayRegistry {
    let mut sessions = vec![
        DashboardSessionInput::new(catalog("root", None), Vec::new()),
        DashboardSessionInput::new(catalog("child-a", Some("root")), Vec::new()),
    ];
    if include_sibling {
        sessions.push(DashboardSessionInput::new(
            catalog("child-b", Some("root")),
            Vec::new(),
        ));
    }
    if include_grandchild {
        sessions.push(DashboardSessionInput::new(
            catalog("grandchild", Some("child-a")),
            Vec::new(),
        ));
    }
    DashboardReplayRegistry::from_sessions(sessions)
}

fn details() -> DashboardDetails {
    DashboardDetails::new(
        &registry(true, true),
        &DashboardEligibilityRules::default(),
        SelectionKey::new("root"),
        RosterState::new(SelectionKey::new("root"), "active", 4, "draft"),
    )
    .unwrap_or_abort()
}

#[test]
fn details_fields_include_metadata_lineage_status_and_activity() {
    let mut details = details();
    details
        .attach(&SelectionKey::new("child-a"))
        .unwrap_or_abort();

    let fields = details.fields().unwrap_or_abort();
    assert_eq!(fields.session_id.as_str(), "child-a");
    assert_eq!(fields.title.as_deref(), Some("Session child-a"));
    assert_eq!(
        fields.metadata.workspace_root.as_deref(),
        Some("/workspace")
    );
    assert_eq!(
        fields.metadata.provider_model.as_deref(),
        Some("mock/model")
    );
    assert_eq!(
        fields.parent.as_ref().map(SelectionKey::as_str),
        Some("root")
    );
    assert_eq!(fields.lineage_depth, 1);
    assert!(fields.is_child);
    assert_eq!(
        fields.status,
        harness_tui::dashboard::DashboardStatus::Running
    );
    assert_eq!(fields.activity.last_event_seq, 0);
}

#[test]
fn nested_attach_cycle_and_back_restore_each_stable_roster_snapshot() {
    let mut details = details();
    details
        .attach(&SelectionKey::new("child-a"))
        .unwrap_or_abort();
    details.set_roster_state(RosterState::new(
        SelectionKey::new("child-a"),
        "child-filter",
        8,
        "child draft",
    ));
    details
        .cycle_related(CycleDirection::Next)
        .unwrap_or_abort();
    assert_eq!(details.current_session_id().as_str(), "child-b");
    details
        .cycle_related(CycleDirection::Previous)
        .unwrap_or_abort();
    assert_eq!(details.current_session_id().as_str(), "child-a");
    details
        .cycle_related(CycleDirection::Next)
        .unwrap_or_abort();
    assert_eq!(details.current_session_id().as_str(), "child-b");
    details
        .attach(&SelectionKey::new("grandchild"))
        .unwrap_or_abort();

    details.back().unwrap_or_abort();
    assert_eq!(details.current_session_id().as_str(), "child-b");
    assert_eq!(details.roster_state().filter, "child-filter");
    assert_eq!(details.roster_state().scroll, 8);
    assert_eq!(details.roster_state().draft, "child draft");

    details.back().unwrap_or_abort();
    assert_eq!(details.current_session_id().as_str(), "root");
    assert_eq!(
        details
            .roster_state()
            .selected_id
            .as_ref()
            .map(SelectionKey::as_str),
        Some("root")
    );
    assert_eq!(details.roster_state().filter, "active");
}

#[test]
fn stale_attach_and_missing_current_are_typed_and_non_mutating() {
    let mut details = details();
    let missing = SelectionKey::new("gone");
    assert_eq!(
        details.attach(&missing),
        Err(NavigationError::MissingSession(missing.clone()))
    );
    assert_eq!(details.current_session_id().as_str(), "root");

    details
        .attach(&SelectionKey::new("child-b"))
        .unwrap_or_abort();
    details
        .refresh(
            &registry(true, false),
            &DashboardEligibilityRules::default(),
        )
        .unwrap_or_abort();
    assert_eq!(
        details.fields(),
        Err(NavigationError::StaleSession(SelectionKey::new("child-b")))
    );
    assert_eq!(
        details.cycle_related(CycleDirection::Next),
        Err(NavigationError::StaleSession(SelectionKey::new("child-b")))
    );
}

#[test]
fn details_replace_roster_on_narrow_and_overlay_on_wide_resize() {
    let details = details();
    let narrow = details.layout_for(Rect::new(0, 0, 120, 40));
    assert_eq!(narrow.mode, DetailsLayoutMode::Replacement);
    assert!(narrow.roster.is_none());

    let wide = details.layout_for(Rect::new(0, 0, 121, 40));
    assert_eq!(wide.mode, DetailsLayoutMode::Overlay);
    assert!(wide.roster.is_some());
    assert!(wide.details.width < 121);
    assert!(wide.details.height <= 40);
    assert_eq!(details.current_session_id().as_str(), "root");
}

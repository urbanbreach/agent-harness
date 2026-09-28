fn permission_event(question: bool) -> harness_core::event::EventEnvelopeV1 {
    use harness_core::event::*;
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION, event_id: "permission-event".into(), seq: 10,
        run_id: "layout-oracle".into(), mono_ms: 100_000, ts: None,
        actor: EventActor::new(ActorKind::System, None), correlation_id: None,
        causation_id: None, stream_key: None,
        payload: EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "permission".into(), kind: if question {"question"} else {"edit_fs"}.into(),
            tool_call_id: Some("tool".into()),
            summary: if question {serde_json::json!({"questions":[{"question":"Which option should be used?","header":"Choice","options":[{"label":"A","description":"first choice with some wrapped text"},{"label":"界B","description":"second choice"}],"multiple":false,"custom":true}]}).to_string()} else {"Edit a document".into()},
            request_digest: "digest".into(), timeout_ms: 30_000, default_decision: PermissionDecision::Deny,
        }),
    }
}

#[test]
fn frozen_frame_plan_matches_replacement() {
    use live_dock_test_fixtures::*;
    let child_dir = tempfile::tempdir().unwrap_or_abort();
    std::fs::write(child_dir.path().join("meta.json"), r#"{"harness_lineage":{"relationship":"task_child_session","parent_run_id":"parent"}}"#).unwrap_or_abort();
    let mut apps = vec![
        AppState::new_startup(Vec::new(),None), AppState::new_live(None,false,None),
        waiting_app(), completed_app(), failed_app(), cancelled_app(), permission_app(),
        AppState::new_replay("/tmp/layout-empty-replay".into(),Vec::new()),
    ];
    for replay in [false,true] {
      for permission in [false,true] {
        let mut app=if replay {AppState::new_replay(child_dir.path().into(),Vec::new())} else {AppState::new_live(Some(child_dir.path().into()),false,None)};
        assert!(app.current_subagent_session_present());
        if permission {app.ingest_event(permission_event(false));}
        apps.push(app);
      }
    }
    for replay in [false,true] {
      let mut app=waiting_app();app.replay_mode=replay;
      app.live_details_drawer_open=true;app.terminal_panel.visible=true;app.todo_pane.visible=true;apps.push(app);
    }
    let mut app=waiting_app();app.ingest_event(permission_event(true));apps.push(app);
    let mut app=AppState::new_live(None,false,None);app.model_prompt_notice=Some("Model notice with wrapping 界 👩‍💻 text".into());apps.push(app);
    for startup in [false,true] {
      let mut app=if startup {AppState::new_startup(Vec::new(),None)}else{waiting_app()};
      app.composer.prompt_buffer="long line 界👩‍💻e\u{301} text ".repeat(20)+"\nsecond\n";apps.push(app);
    }
    let mut app=waiting_app();app.handle_key(crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('p'),crossterm::event::KeyModifiers::CONTROL));apps.push(app);
    let mut app=waiting_app();app.handle_key(crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('/'),crossterm::event::KeyModifiers::NONE));apps.push(app);
    for replay in [false, true] {
      let mut app = completed_app(); app.replay_mode = replay;
      app.active_review_surface = Some(crate::app::ReviewSurface::Help); apps.push(app);
    }
    use crate::rewind_view::{RewindPhase, RewindState, RewindPointInfo};
    for phase in [RewindPhase::Loading, RewindPhase::Picker {points: Vec::new(), selected: 0},
        RewindPhase::Picker {points: (0..20).map(|prompt_index| RewindPointInfo {prompt_index, created_at: String::new(), num_file_snapshots: 0, prompt_preview: Some("saved prompt".into()), has_file_changes: false}).collect(), selected: 5},
        RewindPhase::CancelOffer {active_idx: 0}, RewindPhase::Confirm {target_prompt_index: 0, active_idx: 0, prompt_preview: Some("saved prompt".into())},
        RewindPhase::Executing {target_prompt_index: 0}, RewindPhase::Error {message: "fixture error".into()}] {
      let mut app = completed_app();
      app.rewind.state = Some(RewindState {phase, anchor_entry_idx: 0, stashed_draft: None, selected_prompt_index: None}); apps.push(app);
    }
    let mut custom=Theme::default();custom.live_shell.primary.content_margin_x=10;custom.live_shell.primary.details_sidebar_width=36;
    custom.live_shell.heights.header=3;custom.live_shell.heights.footer=4;custom.live_shell.heights.status=2;
    let themes=[Theme::default(),Theme::harness_light(),custom];
    let mut areas=Vec::new();
    for width in [0,1,2,4,5,6,23,24,59,60,61,79,80,81,99,100,120,121,140,160] {
      for height in [0,1,2,3,4,5,6,7,8,12,17,18,19,20,21,23,24,25,30,40,50] {
        for (x,y) in [(0,0),(7,11)] {areas.push(Rect::new(x,y,width,height));}
      }
    }
    for (x,y) in [(65530,65530),(65535,65535),(65530,7),(7,65530)] {
      for (width,height) in [(0,0),(1,1),(4,4),(5,5),(12,12),(80,24)] {areas.push(Rect{x,y,width,height});}
    }
    let mut cases=0;
    for (state,app) in apps.iter_mut().enumerate() {
      for (theme_index,theme) in themes.into_iter().enumerate() {
        app.set_theme_for_test(theme);
        for area in &areas {
          let expected=FrameLayoutPlan::for_app(app,*area);
          let actual=crate::layout::FrameLayoutPlan::for_app(app,*area);
          assert_eq!(format!("{expected:?}"),format!("{actual:?}"),"state={state},theme={theme_index},area={area:?}");
          cases+=1;
        }
      }
    }
    println!("Compared {cases} complete frame plans: {} states, {} themes, {} rectangles.",apps.len(),themes.len(),areas.len());
}

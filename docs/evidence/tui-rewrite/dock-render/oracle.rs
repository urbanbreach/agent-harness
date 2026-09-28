use crate::{app::AppState, theme::Theme, UnwrapOrAbort};
use ratatui::backend::Backend;
use std::io::Write;
#[path = "/home/urbanbreach/Projects/agent-harness/crates/harness-tui/src/layout_live_dock_test_fixtures.rs"]
mod live_dock_test_fixtures;
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
fn dock_render_reference() {
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
    for active in [false, true] {
      for draft in ["", "draft 界👩‍💻", "first\nsecond"] {
        let mut app=if active {waiting_app()} else {completed_app()};
        app.composer.prompt_buffer=draft.into();
        apps.push(app);
      }
    }
    for count in [0, 2] {
      let mut app=completed_app();
      app.projection.compaction_usage_metrics=crate::app::CompactionUsageMetrics {completed_count:count,summary_tokens_estimate:1400,reduction_tokens_estimate:12000,last_reduction_percent_estimate:Some(45),..Default::default()};
      app.projection.compaction_status=Some(crate::app::CompactionStatus {agent_id:"main".into(),checkpoint_id:None,trigger_reason:"test".into(),state:crate::app::CompactionState::Requested,message:"Compacting 界👩‍💻 context".into()});
      apps.push(app);
    }
    let mut app=completed_app();app.composer.prompt_buffer="clear me".into();
    app.handle_key(crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Esc,crossterm::event::KeyModifiers::NONE));apps.push(app);
    let mut app=completed_app();app.todo_pane.visible=true;app.todo_pane.focused=true;app.focus=crate::app::Focus::Details;apps.push(app);
    let mut app=failed_app();app.replay_mode=true;apps.push(app);
    for status in ["disconnected", "lagged 界👩‍💻 stream"] {
      let mut app=completed_app();app.set_status_banner(Some(status.into()));apps.push(app);
    }
    for draft in ["", "draft", "first\nsecond"] {
      let mut app=waiting_app();app.composer.prompt_buffer=draft.into();
      app.apply_keybindings([("submit_prompt".into(),"F12".into()),("insert_newline".into(),"Ctrl+Up".into()),("help".into(),"Alt+F11".into()),("variant_cycle".into(),"Ctrl+F10".into())].into());apps.push(app);
    }
    for remapped in [false,true] {
      let mut app=waiting_app();app.composer.multiline_mode=true;
      app.composer.prompt_buffer="first\nsecond".into();
      if remapped {app.apply_keybindings([("interject_prompt".into(),"F11".into()),("cancel_and_replace_prompt".into(),"F12".into())].into());}
      assert!(app.composer.composer_multiline_mode() && app.active_turn_in_progress());apps.push(app);
    }
    for clear in [false,true] {
      let mut app=AppState::new_live(None,false,None);app.set_starting_session_seed(true);
      if clear {app.composer.prompt_buffer="clear seed".into();app.handle_key(crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Esc,crossterm::event::KeyModifiers::NONE));assert!(app.clear_prompt_confirmation_pending());}
      assert!(app.starting_session_seed_visible());apps.push(app);
    }
    for count in [1,2] {
      for context in [false,true] {
        let mut app=AppState::new_live(None,false,None);
        for seq in 1..=count {
          let mut event=permission_event(false);event.seq=seq;
          event.payload=harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id:format!("background-{seq}").into(),state:harness_core::event::TaskScheduleState::Started,
            queue_key:Some("background:analysis".into()),metadata:None,
          });app.ingest_event(event);
        }
        if context {app.projection.compaction_usage_metrics.completed_count=1;}
        assert_eq!(app.active_background_task_count(),count as usize);assert!(!app.active_turn_in_progress());apps.push(app);
      }
    }
    let mut custom=Theme::default();custom.live_shell.primary.content_margin_x=10;custom.live_shell.primary.details_sidebar_width=36;
    custom.live_shell.heights.header=3;custom.live_shell.heights.footer=4;custom.live_shell.heights.status=2;
    let themes=[Theme::default(),Theme::harness_light(),Theme::terminal_native(),custom];
    let path=std::env::var("HARNESS_DOCK_ORACLE_OUT").unwrap_or_abort();
    let mut output=std::io::BufWriter::new(std::fs::File::create(path).unwrap_or_abort());
    let mut cases=0;
    for (state,app) in apps.iter_mut().enumerate() {
      let now=std::time::Instant::now();
      app.set_now_fn_for_test(std::sync::Arc::new(move || now));
      app.live_turn_started_at=app.live_turn_started_at.map(|_| now-std::time::Duration::from_millis(1250));
      app.live_turn_phase_started_at=app.live_turn_phase_started_at.map(|_| now-std::time::Duration::from_millis(450));
      app.clear_prompt_confirm_deadline=app.clear_prompt_confirm_deadline.map(|_| now+std::time::Duration::from_secs(3));
      app.restart_motion_epoch_for_evidence();
      app.set_reduced_motion(true);
      for (theme_index,theme) in themes.into_iter().enumerate() {
        app.set_theme_for_test(theme);
        for width in [1,2,5,12,20,39,40,59,60,61,79,80,81,99,100,119,120,140,160] {
          for height in [6,20,40] {
            let mut terminal=ratatui::Terminal::new(ratatui::backend::TestBackend::new(width,height)).unwrap_or_abort();
            terminal.draw(|frame| crate::ui::render_app(frame,app)).unwrap_or_abort();
            let mut cells:Vec<(usize,String)>=Vec::new();
            for cell in &terminal.backend().buffer().content {
              let text=format!("{cell:?}");
              if let Some((count,_previous))=cells.last_mut().filter(|(_,previous)|*previous==text) {*count+=1;}else{cells.push((1,text));}
            }
            let cursor=terminal.backend_mut().get_cursor_position().unwrap_or_abort();
            let record=serde_json::json!({"state":state,"theme":theme_index,"width":width,"height":height,"cells":cells,"cursor":format!("{cursor:?}"),"context":[app.runtime_context_primary_summary(),app.runtime_context_summary_segment_text().unwrap_or_default(),app.runtime_context_provider_display().unwrap_or_default()]});
            writeln!(output,"{record}").unwrap_or_abort();cases+=1;
          }
        }
      }
    }
    println!("Recorded {cases} exact full styled buffers and cursors for {} states, {} themes, 57 dimensions.",apps.len(),themes.len());
}

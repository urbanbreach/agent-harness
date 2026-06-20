use super::*;

pub(super) fn typing_at_opens_file_mention_menu_with_directories() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(tempdir.path().join("src")).expect("create src");
    std::fs::write(tempdir.path().join("src/lib.rs"), "lib").expect("write lib");
    std::fs::write(tempdir.path().join("README.md"), "readme").expect("write readme");

    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());
    app.focus = Focus::Prompt;

    app.handle_key(key(KeyCode::Char('@')));

    assert!(app.file_mention_overlay_should_render());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::FileMentions));
    assert_eq!(app.file_mention_entries[0].display, "src/");
}

pub(super) fn file_mention_tab_expands_directory_without_closing_menu() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(tempdir.path().join("src/bin")).expect("create nested dir");
    std::fs::write(tempdir.path().join("src/main.rs"), "fn main() {}").expect("write main");

    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());
    app.focus = Focus::Prompt;
    app.handle_key(key(KeyCode::Char('@')));

    app.handle_key(key(KeyCode::Tab));

    assert_eq!(app.composer.prompt_buffer, "@src/");
    assert!(app.file_mention_overlay_should_render());
    assert!(app
        .file_mention_entries
        .iter()
        .any(|entry| entry.display == "src/main.rs"));
}

pub(super) fn file_mention_enter_inserts_selected_file_with_space() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(tempdir.path().join("src")).expect("create src");
    std::fs::write(tempdir.path().join("src/main.rs"), "fn main() {}").expect("write main");

    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());
    app.focus = Focus::Prompt;
    for ch in "@main".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.composer.prompt_buffer, "@src/main.rs ");
    assert_eq!(app.file_mention_tags.len(), 1);
    assert_eq!(app.file_mention_tags[0].start, 0);
    assert_eq!(app.file_mention_tags[0].end, "@src/main.rs".chars().count());
    let selected = app.selected_file_tags();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].path, "src/main.rs");
    assert_eq!(selected[0].filename, "src/main.rs");
    assert_eq!(selected[0].mime, "text/plain");
    assert!(selected[0].url.ends_with("/src/main.rs"));
    assert!(!app.file_mention_overlay_should_render());
}

pub(super) fn file_mentions_use_injected_scanner_workspace_and_clock() {
    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_collaborators_for_test(
        PathBuf::from("/virtual/workspace"),
        vec!["docs/main.rs".to_string(), "src/main.rs".to_string()],
        123,
    );
    app.focus = Focus::Prompt;

    for ch in "@src/main".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    let selected = app.selected_file_tags();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].path, "src/main.rs");
    assert_eq!(selected[0].url, "file:///virtual/workspace/src/main.rs");
    assert_eq!(
        app.file_mention_frecency_for_test("src/main.rs"),
        Some((1, 123))
    );

    app.clear_prompt_input();
    for ch in "@main".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(app.file_mention_entries[0].display, "src/main.rs");
}

pub(super) fn submitting_selected_file_mention_emits_structured_file_part() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(tempdir.path().join("src")).expect("create src");
    std::fs::write(tempdir.path().join("src/main.rs"), "fn main() {}").expect("write main");
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent| intents.lock().expect("lock intents").push(intent))
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());
    app.focus = Focus::Prompt;
    for ch in "@main".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    let [UiIntent::SubmitPrompt {
        text,
        selected_file_tags,
        ..
    }] = intents.as_slice()
    else {
        panic!("expected one submit prompt intent: {intents:?}");
    };
    assert_eq!(text, "@src/main.rs ");
    assert_eq!(selected_file_tags.len(), 1);
    assert_eq!(selected_file_tags[0].path, "src/main.rs");
    assert_eq!(selected_file_tags[0].source.value, "@src/main.rs");
}

pub(super) fn file_mention_picker_selects_agent_parts_from_launch_metadata() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-1").with_available_models(vec![
            ModelOption::from_model_ref("build", "mock:model-1"),
            ModelOption::from_model_ref("plan", "mock:model-1"),
            ModelOption::from_model_ref(
                harness_core::session_title::TITLE_AGENT_NAME,
                "mock:model-1",
            ),
        ]),
    );
    app.focus = Focus::Prompt;

    for ch in "@pla".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.composer.prompt_buffer, "@plan ");
    assert!(app.selected_file_tags().is_empty());
    assert!(app.selected_resource_tags().is_empty());
    let selected = app.selected_agent_tags();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].name, "plan");
    assert_eq!(selected[0].source.value, "@plan");
}

pub(super) fn file_mention_picker_selects_mcp_resource_parts_from_launch_metadata() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-1").with_mcp_resources(vec![
            McpResourceOption {
                name: "Docs Guide".to_string(),
                uri: "mcp://docs/guide".to_string(),
                mime: "text/markdown".to_string(),
                description: Some("Documentation index".to_string()),
            },
        ]),
    );
    app.focus = Focus::Prompt;

    for ch in "@guide".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.composer.prompt_buffer, "@mcp://docs/guide ");
    assert!(app.selected_file_tags().is_empty());
    assert!(app.selected_agent_tags().is_empty());
    let selected = app.selected_resource_tags();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].name, "Docs Guide");
    assert_eq!(selected[0].uri, "mcp://docs/guide");
    assert_eq!(selected[0].mime, "text/markdown");
    assert_eq!(
        selected[0].description.as_deref(),
        Some("Documentation index")
    );
    assert_eq!(selected[0].source.value, "@mcp://docs/guide");
}

pub(super) fn file_mention_tag_is_removed_when_user_edits_inside_it() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(tempdir.path().join("src")).expect("create src");
    std::fs::write(tempdir.path().join("src/main.rs"), "fn main() {}").expect("write main");

    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());
    app.focus = Focus::Prompt;
    for ch in "@main".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    app.composer.prompt_cursor = 2;
    app.handle_key(key(KeyCode::Char('x')));

    assert!(app.file_mention_tags.is_empty());
}

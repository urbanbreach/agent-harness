#[tokio::test]
async fn native_bash_accepts_baseline_shape_without_description() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let bash = registry.get("bash").expect("bash in registry");

    // act
    let result = bash
        .call(
            test_context(workspace, "bash-baseline-shape"),
            json!({
                "command": "printf ok"
            }),
        )
        .await
        .expect("baseline bash shape should not require Harness-only description");

    // assert
    assert!(result.display_text.contains("ok"));
}

#[tokio::test]
async fn native_glob_accepts_baseline_limit_shape() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let glob = registry.get("glob").expect("glob in registry");

    fs::write(workspace.join("a.rs"), "fn a() {}\n").expect("write a.rs");
    fs::write(workspace.join("b.rs"), "fn b() {}\n").expect("write b.rs");

    // act
    let result = glob
        .call(
            test_context(workspace, "glob-baseline-limit"),
            json!({
                "pattern": "*.rs",
                "limit": 1
            }),
        )
        .await
        .expect("baseline glob shape should accept limit");

    // assert
    assert!(
        glob.parameters_json_schema()
            .pointer("/properties/limit")
            .is_some(),
        "provider-visible glob schema should advertise the current upstream limit field"
    );
    let structured = result.structured_json.expect("glob structured json");
    assert_eq!(structured["returned_count"], json!(1));
    assert_eq!(structured["truncated"], json!(true));
}

#[tokio::test]
async fn native_read_directory_uses_baseline_offset_and_guidance() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").expect("read in registry");

    let docs = workspace.join("docs");
    fs::create_dir(&docs).expect("create docs dir");
    fs::write(docs.join("a.txt"), "a\n").expect("write a");
    fs::write(docs.join("b.txt"), "b\n").expect("write b");
    fs::write(docs.join("c.txt"), "c\n").expect("write c");

    // act
    let result = read
        .call(
            test_context(workspace, "read-directory-offset"),
            json!({
                "filePath": "docs",
                "offset": 2,
                "limit": 1
            }),
        )
        .await
        .expect("directory read should accept offset and limit");

    // assert
    assert!(result.display_text.contains("<type>directory</type>"));
    assert!(result.display_text.contains("\nb.txt\n"));
    assert!(!result.display_text.contains("\na.txt\n"));
    assert!(result
        .display_text
        .contains("Use 'offset' parameter to read beyond entry 3"));
    let structured = result.structured_json.expect("directory read metadata");
    assert_eq!(structured["entries"], json!(["b.txt"]));
    assert_eq!(structured["metadata"]["display"]["offset"], json!(2));
    assert_eq!(structured["truncated"], json!(true));
}

#[tokio::test]
async fn native_grep_accepts_baseline_limit_shape() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let grep = registry.get("grep").expect("grep in registry");

    fs::write(workspace.join("grep.txt"), "needle one\nneedle two\n").expect("write grep file");

    // act
    let result = grep
        .call(
            test_context(workspace, "grep-baseline-limit"),
            json!({
                "pattern": "needle",
                "path": "grep.txt",
                "limit": 1
            }),
        )
        .await
        .expect("baseline grep shape should accept limit");

    // assert
    let structured = result.structured_json.expect("grep structured json");
    assert_eq!(structured["limit"], json!(1));
    assert_eq!(structured["returned_count"], json!(1));
    assert_eq!(structured["truncated"], json!(true));
}

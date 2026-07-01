#[tokio::test]
async fn native_apply_patch_rejects_later_move_before_earlier_add_mutates() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let apply_patch = registry.get("apply_patch").expect("apply_patch in registry");

    fs::write(workspace.join("source.txt"), "alpha\nbeta\n").expect("seed source file");

    // act
    let error = apply_patch
        .call(
            test_context(workspace, "apply-patch-add-then-move"),
            json!({
                "patchText": "*** Begin Patch\n*** Add File: added.txt\n+new file\n*** Update File: source.txt\n*** Move to: moved.txt\n@@\n alpha\n-beta\n+BETA\n*** End Patch"
            }),
        )
        .await
        .expect_err("move should fail during preflight");

    // assert
    assert!(
        error
            .to_string()
            .contains("apply_patch moves are not supported yet"),
        "unexpected error: {error}"
    );
    assert!(!workspace.join("added.txt").exists());
    assert_eq!(
        fs::read_to_string(workspace.join("source.txt")).expect("read source file"),
        "alpha\nbeta\n"
    );
}

#[tokio::test]
async fn native_apply_patch_rejects_later_missing_update_before_earlier_add_mutates() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let apply_patch = registry.get("apply_patch").expect("apply_patch in registry");

    // act
    let error = apply_patch
        .call(
            test_context(workspace, "apply-patch-add-then-missing-update"),
            json!({
                "patchText": "*** Begin Patch\n*** Add File: added.txt\n+new file\n*** Update File: missing.txt\n@@\n-old\n+new\n*** End Patch"
            }),
        )
        .await
        .expect_err("missing update should fail during preflight");

    // assert
    assert!(
        error.to_string().contains("Unable to apply patch at missing.txt"),
        "unexpected error: {error}"
    );
    assert!(!workspace.join("added.txt").exists());
}

#[tokio::test]
async fn native_apply_patch_rejects_later_missing_delete_before_earlier_add_mutates() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let apply_patch = registry.get("apply_patch").expect("apply_patch in registry");

    // act
    let error = apply_patch
        .call(
            test_context(workspace, "apply-patch-add-then-missing-delete"),
            json!({
                "patchText": "*** Begin Patch\n*** Add File: added.txt\n+new file\n*** Delete File: missing.txt\n*** End Patch"
            }),
        )
        .await
        .expect_err("missing delete should fail during preflight");

    // assert
    assert!(
        error.to_string().contains("Unable to apply patch at missing.txt"),
        "unexpected error: {error}"
    );
    assert!(!workspace.join("added.txt").exists());
}

#[tokio::test]
async fn native_apply_patch_rejects_later_traversal_target_before_earlier_add_mutates() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let apply_patch = registry.get("apply_patch").expect("apply_patch in registry");

    // act
    let error = apply_patch
        .call(
            test_context(workspace, "apply-patch-add-then-traversal"),
            json!({
                "patchText": "*** Begin Patch\n*** Add File: added.txt\n+new file\n*** Add File: ../escape.txt\n+escape\n*** End Patch"
            }),
        )
        .await
        .expect_err("traversal should fail during preflight");

    // assert
    assert!(
        error.to_string().contains("path escapes workspace root"),
        "unexpected error: {error}"
    );
    assert!(!workspace.join("added.txt").exists());
}

#[tokio::test]
async fn native_apply_patch_matches_baseline_context_and_normalized_text() {
    // arrange
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let apply_patch = registry.get("apply_patch").expect("apply_patch in registry");

    fs::write(workspace.join("decorator.txt"), "@decorator\nsay “hello”\n")
        .expect("seed decorator file");

    // act
    apply_patch
        .call(
            test_context(workspace, "apply-patch-context-normalized"),
            json!({
                "patchText": "*** Begin Patch\n*** Update File: decorator.txt\n@@ @decorator\n-say \"hello\"\n+say \"hi\"\n*** End Patch"
            }),
        )
        .await
        .expect("context header and normalized quotes should match baseline behavior");

    // assert
    assert_eq!(
        fs::read_to_string(workspace.join("decorator.txt")).expect("read decorator file"),
        "@decorator\nsay \"hi\"\n"
    );
}

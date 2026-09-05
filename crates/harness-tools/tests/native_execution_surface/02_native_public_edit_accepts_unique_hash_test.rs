use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn native_public_edit_accepts_unique_hash_only_anchor() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").unwrap_or_abort();

    fs::write(workspace.join("surface.txt"), "current\nnext\n").unwrap_or_abort();

    let result = edit
        .call(
            test_context(workspace, "edit-hash-only-anchor"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("#{}", compute_line_hash("current")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .unwrap_or_abort();

    assert!(result.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).unwrap_or_abort(),
        "after\nnext\n"
    );
}
#[tokio::test]
async fn native_public_edit_uses_recent_hashline_read_to_disambiguate_hash_only_anchor() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").unwrap_or_abort();
    let edit = registry.get("edit").unwrap_or_abort();
    let tool_state = ToolRunState::default();

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").unwrap_or_abort();

    read.call(
        test_context_with_tool_state(workspace, "read-disambiguation-window", tool_state.clone()),
        json!({
            "filePath": "surface.txt",
            "offset": 1,
            "limit": 2,
        }),
    )
    .await
    .unwrap_or_abort();

    let result = edit
        .call(
            test_context_with_tool_state(
                workspace,
                "edit-read-window-hash-only-anchor",
                tool_state,
            ),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("#{}", compute_line_hash("same")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .unwrap_or_abort();

    assert!(result.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).unwrap_or_abort(),
        "after\nother\nsame\n"
    );
}
#[tokio::test]
async fn native_public_edit_scopes_recent_hashline_reads_to_shared_tool_run_state_not_run_id() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").unwrap_or_abort();
    let edit = registry.get("edit").unwrap_or_abort();
    let tool_state = ToolRunState::default();

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").unwrap_or_abort();

    read.call(
        common_test_context_with_tool_state(
            workspace,
            "read-owner-run",
            "read-shared-edit-session-owner",
            tool_state.clone(),
        ),
        json!({
            "filePath": "surface.txt",
            "offset": 1,
            "limit": 2,
        }),
    )
    .await
    .unwrap_or_abort();

    let result = edit
        .call(
            common_test_context_with_tool_state(
                workspace,
                "edit-owner-run",
                "edit-shared-edit-session-owner",
                tool_state,
            ),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("#{}", compute_line_hash("same")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .unwrap_or_abort();

    assert!(result.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).unwrap_or_abort(),
        "after\nother\nsame\n"
    );
}
#[tokio::test]
async fn native_internal_hashline_scan_disambiguates_hash_only_anchor_for_edit() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry_with_internal_hashline_tools(ShellAllowlist::default());
    let scan = registry
        .get("edit.hashline_scan")
        .unwrap_or_abort();
    let edit = registry.get("edit").unwrap_or_abort();
    let tool_state = ToolRunState::default();

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").unwrap_or_abort();

    scan.call(
        test_context_with_tool_state(workspace, "scan-disambiguation-window", tool_state.clone()),
        json!({
            "path": "surface.txt",
            "start_line": 1,
            "limit": 2,
        }),
    )
    .await
    .unwrap_or_abort();

    let result = edit
        .call(
            test_context_with_tool_state(
                workspace,
                "edit-scan-window-hash-only-anchor",
                tool_state,
            ),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("#{}", compute_line_hash("same")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .unwrap_or_abort();

    assert!(result.display_text.contains("Edit applied successfully"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).unwrap_or_abort(),
        "after\nother\nsame\n"
    );
}
#[tokio::test]
async fn native_public_edit_ignores_stale_recent_hashline_read_for_hash_only_anchor() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").unwrap_or_abort();
    let edit = registry.get("edit").unwrap_or_abort();
    let tool_state = ToolRunState::default();

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").unwrap_or_abort();

    read.call(
        test_context_with_tool_state(
            workspace,
            "read-stale-disambiguation-window",
            tool_state.clone(),
        ),
        json!({
            "filePath": "surface.txt",
            "offset": 1,
            "limit": 2,
        }),
    )
    .await
    .unwrap_or_abort();

    fs::write(workspace.join("surface.txt"), "same\nanother\nsame\n")
        .unwrap_or_abort();

    let error = edit
        .call(
            test_context_with_tool_state(
                workspace,
                "edit-stale-read-window-hash-only-anchor",
                tool_state,
            ),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("#{}", compute_line_hash("same")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect_err("stale cached anchors should not disambiguate hash-only anchor");

    let error = error.to_string();
    assert!(error.contains("matches multiple current lines"));
    assert!(error.contains("Re-read the file"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).unwrap_or_abort(),
        "same\nanother\nsame\n"
    );
}
#[tokio::test]
async fn native_public_edit_does_not_share_recent_hashline_reads_across_tool_state() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").unwrap_or_abort();
    let edit = registry.get("edit").unwrap_or_abort();

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").unwrap_or_abort();

    read.call(
        test_context_with_tool_state(
            workspace,
            "read-isolated-disambiguation-window",
            ToolRunState::default(),
        ),
        json!({
            "filePath": "surface.txt",
            "offset": 1,
            "limit": 2,
        }),
    )
    .await
    .unwrap_or_abort();

    let error = edit
        .call(
            test_context_with_tool_state(
                workspace,
                "edit-isolated-hash-only-anchor",
                ToolRunState::default(),
            ),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("#{}", compute_line_hash("same")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect_err("separate tool state must not disambiguate hash-only anchor");

    let error = error.to_string();
    assert!(error.contains("matches multiple current lines"));
    assert!(error.contains("Re-read the file"));
    assert_eq!(
        fs::read_to_string(workspace.join("surface.txt")).unwrap_or_abort(),
        "same\nother\nsame\n"
    );
}
#[tokio::test]
async fn native_public_edit_rejects_ambiguous_hash_only_anchor() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").unwrap_or_abort();

    fs::write(workspace.join("surface.txt"), "same\nother\nsame\n").unwrap_or_abort();

    let error = edit
        .call(
            test_context(workspace, "edit-ambiguous-hash-only-anchor"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("#{}", compute_line_hash("same")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect_err("ambiguous hash-only anchor should fail");

    let error = error.to_string();
    assert!(error.contains("omitted its line number and matches multiple current lines"));
    assert!(error.contains("Re-read the file"));
    assert!(error.contains(&format!(">>> 1#{}|same", compute_line_hash("same"))));
    assert!(error.contains(&format!(">>> 3#{}|same", compute_line_hash("same"))));
}
#[tokio::test]
async fn native_public_edit_rejects_unknown_hash_only_anchor() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").unwrap_or_abort();

    fs::write(workspace.join("surface.txt"), "same\nother\n").unwrap_or_abort();

    let error = edit
        .call(
            test_context(workspace, "edit-missing-hash-only-anchor"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": "#deadbeefdead",
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect_err("unknown hash-only anchor should fail");

    let error = error.to_string();
    assert!(error.contains("does not match any current line"));
    assert!(error.contains("Re-read the file"));
}
#[tokio::test]
async fn native_public_edit_stale_anchor_error_includes_refresh_snippet() {
    let workspace_fixture = setup_workspace_fixture();
    let workspace = workspace_fixture.workspace();
    let registry = coordinator_registry(ShellAllowlist::default());
    let edit = registry.get("edit").unwrap_or_abort();

    fs::write(workspace.join("surface.txt"), "current\nnext\n").unwrap_or_abort();

    let error = edit
        .call(
            test_context(workspace, "edit-stale"),
            json!({
                "filePath": "surface.txt",
                "edits": [
                    {
                        "op": "replace",
                        "pos": format!("1#{}", compute_line_hash("stale")),
                        "lines": ["after"],
                    }
                ],
            }),
        )
        .await
        .expect_err("stale anchor should fail");

    let error = error.to_string();
    assert!(error.contains("Copy updated tags from this snippet"));
    assert!(error.contains("re-read the file"));
    assert!(error.contains(">>> 1#"));
    assert!(error.contains("|current"));
    assert!(error.contains(">>> 2#"));
}

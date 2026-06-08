use super::super::*;

#[cfg(test)]
pub(crate) fn exact_test_transcript_edit_tool_matches_inline_diff_shape() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    std::fs::write(
        artifacts_dir.join("harness-inline.diff"),
        "--- crates/harness-tui/src/ui.rs\n+++ crates/harness-tui/src/ui.rs\n@@ -44,8 +44,7 @@\n use ui_secondary::{\n-    render_diff_tab, render_events_tab, render_help_tab,\n+    render_events_tab, render_help_tab, render_live_details_overlay,\n     render_operator_sidebar,\n };\n",
    )
    .expect("write inline diff fixture");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry =
        transcript_section_model_test_activity("request-edit-inline", ActivityStatus::Done, "");
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-edit-1".to_string(),
        tool_id: "edit.hashline_apply".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"path":"crates/harness-tui/src/ui.rs"}"#.to_string(),
        args_digest: "digest-edit".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Edit applied".to_string()),
        output_digest: Some("digest-edit-output".to_string()),
        output_json: None,
        truncated_output: Some("Edit applied".to_string()),
        edit: Some(crate::app::EditEntry {
            edit_id: "edit-inline-1".to_string(),
            path: "crates/harness-tui/src/ui.rs".to_string(),
            status: crate::app::EditDisplayStatus::Applied,
            summary: Some("Remove diff review surface".to_string()),
            patch_digest: Some("digest-patch".to_string()),
            new_file_digest: Some("digest-new-file".to_string()),
            diff_rel_path: Some("artifacts/harness-inline.diff".to_string()),
            diff_digest: Some("digest-diff".to_string()),
            rejection_reason: None,
        }),
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 160)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("← Patch · ui.rs"));
    assert!(rendered.contains("crates/harness-tui/src"));
    assert!(rendered.contains("render_diff_tab"));
    assert!(rendered.contains("render_live_details_overlay"));
    assert!(rendered.contains("44"));
    assert!(
        !rendered.contains("@@ -44,8 +44,7 @@"),
        "inline transcript diffs should suppress raw hunk headers to match harness chat diffs\n{rendered}"
    );
    assert!(
        rendered.lines().any(|line| {
            line.contains("render_diff_tab") && line.contains("render_live_details_overlay")
        }),
        "tool inline diff should render side-by-side in wide transcript layouts\n{rendered}"
    );
    assert!(rendered.lines().any(|line| {
        line.contains("render_diff_tab") && line.contains('-') && line.contains('+')
    }));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_native_edit_renders_inline_diff_from_artifact() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    std::fs::write(
        artifacts_dir.join("native-edit.diff"),
        "--- docs/rust.md\n+++ docs/rust.md\n@@ -1,3 +1,2 @@\n # Rust\n-## Ownership\n Safe and fast\n",
    )
    .expect("write native edit diff fixture");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry =
        transcript_section_model_test_activity("request-native-edit", ActivityStatus::Done, "");
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-native-edit-1".to_string(),
        tool_id: "edit".to_string(),
        canonical_tool_id: Some("edit".to_string()),
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary:
            "{\"filePath\":\"docs/rust.md\",\"oldString\":\"## Ownership\\n\",\"newString\":\"\"}"
                .to_string(),
        args_digest: "digest-native-edit".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Edit applied successfully.".to_string()),
        output_digest: Some("digest-native-edit-output".to_string()),
        output_json: None,
        truncated_output: Some("Edit applied successfully.".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: vec![crate::app::ToolArtifactEntry {
            path: "artifacts/native-edit.diff".to_string(),
            digest: Some("digest-native-edit-diff".to_string()),
        }],
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 140)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("Edit · rust.md"));
    assert!(rendered.contains("docs"));
    assert!(rendered.contains("Ownership"));
    assert!(rendered
        .lines()
        .any(|line| line.contains("Ownership") && line.contains('-')));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_apply_patch_multifile_uses_output_edit_paths() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    std::fs::write(
        artifacts_dir.join("apply-a.diff"),
        "@@ -1,1 +1,1 @@\n-old a\n+new a\n",
    )
    .expect("write apply patch diff fixture a");
    std::fs::write(
        artifacts_dir.join("apply-b.diff"),
        "@@ -1,1 +1,1 @@\n-old b\n+new b\n",
    )
    .expect("write apply patch diff fixture b");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry = transcript_section_model_test_activity(
        "request-apply-patch-inline",
        ActivityStatus::Done,
        "",
    );
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-apply-patch-1".to_string(),
        tool_id: "apply_patch".to_string(),
        canonical_tool_id: Some("apply_patch".to_string()),
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"patchText":"*** Begin Patch"}"#.to_string(),
        args_digest: "digest-apply-patch".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Success. Updated the following files".to_string()),
        output_digest: Some("digest-apply-patch-output".to_string()),
        output_json: Some(serde_json::json!({
            "files": ["M notes/a.md", "M notes/b.md"],
            "edits": [
                {
                    "edit_id": "apply-patch-1",
                    "path": "notes/a.md",
                    "summary": "apply patch update notes/a.md",
                    "deleted": false,
                    "diff_rel_path": "artifacts/apply-a.diff",
                    "diff_digest": "digest-apply-a"
                },
                {
                    "edit_id": "apply-patch-2",
                    "path": "notes/b.md",
                    "summary": "apply patch update notes/b.md",
                    "deleted": false,
                    "diff_rel_path": "artifacts/apply-b.diff",
                    "diff_digest": "digest-apply-b"
                }
            ]
        })),
        truncated_output: Some("Success. Updated the following files".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: vec![
            crate::app::ToolArtifactEntry {
                path: "artifacts/apply-a.diff".to_string(),
                digest: Some("digest-apply-a".to_string()),
            },
            crate::app::ToolArtifactEntry {
                path: "artifacts/apply-b.diff".to_string(),
                digest: Some("digest-apply-b".to_string()),
            },
        ],
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.set_patch_file_output_expanded_for_test("call-apply-patch-1", "notes/a.md", true);
    app.set_patch_file_output_expanded_for_test("call-apply-patch-1", "notes/b.md", true);
    assert!(app.patch_file_output_expanded("call-apply-patch-1", "notes/a.md"));
    assert!(app.patch_file_output_expanded("call-apply-patch-1", "notes/b.md"));
    let sections = build_transcript_sections(&app);
    let turn = &sections[0];
    let Some(TranscriptToolCallDetailBlock::FileSection(file_section)) =
        turn.tool_calls[0].detail_blocks.first()
    else {
        panic!("expected apply_patch file section");
    };
    assert_eq!(
        file_section.disclosure_state,
        TranscriptToolCallDisclosureState::Expanded
    );
    assert!(!file_section.detail_blocks.is_empty());

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 140)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let direct_rendered = transcript_test_line_texts(
        append_tool_call_section_lines(
            &turn.tool_calls[0],
            &Theme::default(),
            140,
            Theme::default().surface.shell,
        )
        .lines,
    )
    .join("\n");

    assert!(rendered.contains("Patch · 2 files"));
    assert!(!rendered.contains("Patch · 2 files  ▸"));
    assert!(rendered.contains("a.md · notes"));
    assert!(rendered.contains("b.md · notes"));
    assert!(
        rendered.contains("new a"),
        "rendered output missing first diff\nfull:\n{rendered}\n\ndirect:\n{direct_rendered}"
    );
    assert!(
        rendered.contains("new b"),
        "rendered output missing second diff\nfull:\n{rendered}\n\ndirect:\n{direct_rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_apply_patch_surfaces_rename_and_wrapped_inline_diffs() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    std::fs::write(
        artifacts_dir.join("apply-rename.diff"),
        "--- src/session_turn.rs\n+++ src/session_diff.rs\n@@ -1,1 +1,1 @@\n-pub fn render_session_turn_diff() {}\n+pub fn render_session_diff_view() {}\n",
    )
    .expect("write rename diff fixture");
    std::fs::write(
        artifacts_dir.join("apply-wrap.diff"),
        "--- docs/transcript.md\n+++ docs/transcript.md\n@@ -1,1 +1,1 @@\n-session turn diff view keeps the tool row spacing perfectly aligned in every transcript lane for operators reviewing compact windows\n+session turn diff view keeps the tool row spacing perfectly aligned across the transcript surface for operators reviewing compact windows and narrow shells\n",
    )
    .expect("write wrapped diff fixture");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry = transcript_section_model_test_activity(
        "request-apply-patch-rename-wrap",
        ActivityStatus::Done,
        "",
    );
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-apply-patch-rename-wrap".to_string(),
        tool_id: "apply_patch".to_string(),
        canonical_tool_id: Some("apply_patch".to_string()),
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"patchText":"*** Begin Patch"}"#.to_string(),
        args_digest: "digest-apply-patch-rename-wrap".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Success. Updated the following files".to_string()),
        output_digest: Some("digest-apply-patch-rename-wrap-output".to_string()),
        output_json: Some(serde_json::json!({
            "files": ["M src/session_diff.rs", "M docs/transcript.md"],
            "edits": [
                {
                    "edit_id": "apply-patch-rename",
                    "path": "src/session_diff.rs",
                    "summary": "apply patch move src/session_turn.rs -> src/session_diff.rs",
                    "deleted": false,
                    "diff_rel_path": "artifacts/apply-rename.diff",
                    "diff_digest": "digest-apply-rename"
                },
                {
                    "edit_id": "apply-patch-wrap",
                    "path": "docs/transcript.md",
                    "summary": "apply patch update docs/transcript.md",
                    "deleted": false,
                    "diff_rel_path": "artifacts/apply-wrap.diff",
                    "diff_digest": "digest-apply-wrap"
                }
            ]
        })),
        truncated_output: Some("Success. Updated the following files".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: vec![
            crate::app::ToolArtifactEntry {
                path: "artifacts/apply-rename.diff".to_string(),
                digest: Some("digest-apply-rename".to_string()),
            },
            crate::app::ToolArtifactEntry {
                path: "artifacts/apply-wrap.diff".to_string(),
                digest: Some("digest-apply-wrap".to_string()),
            },
        ],
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.set_patch_file_output_expanded_for_test(
        "call-apply-patch-rename-wrap",
        "src/session_diff.rs",
        true,
    );
    app.set_patch_file_output_expanded_for_test(
        "call-apply-patch-rename-wrap",
        "docs/transcript.md",
        true,
    );
    assert!(app.patch_file_output_expanded("call-apply-patch-rename-wrap", "src/session_diff.rs"));
    assert!(app.patch_file_output_expanded("call-apply-patch-rename-wrap", "docs/transcript.md"));
    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        84,
    ));
    let rendered = lines.join("\n");

    let rename_header = lines
        .iter()
        .find(|line| line.contains("session_diff.rs") && line.contains("src"))
        .expect("rename header");
    assert!(
        rename_header.contains("session_diff.rs"),
        "rename header: {rename_header}"
    );
    assert!(rendered.contains("Patch · 2 files"));
    assert!(!rendered.contains("Patch · 2 files  ▸"));
    assert!(
        lines.iter().any(|line| {
            line.contains("session turn diff view keeps the tool row spacing perfectly aligne")
        }),
        "long diff prefix missing\n{rendered}"
    );
    assert!(
        lines.iter().any(|line| {
            line.contains("n every transcript lane for operators reviewing compact windows")
        }),
        "removed line continuation missing\n{rendered}"
    );
    assert!(
        lines.iter().any(|line| {
            line.contains("d across the transcript surface for operators reviewing compact wi")
        }),
        "added line wrapped continuation prefix missing\n{rendered}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("s and narrow shells")),
        "added line wrapped continuation tail missing\n{rendered}"
    );
    assert!(
        !rendered.contains('…'),
        "wrapped transcript diff should not fall back to truncation\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_inline_diff_stays_compact_between_tool_rows() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    std::fs::write(
        artifacts_dir.join("apply-compact.diff"),
        "--- docs/spacing.md\n+++ docs/spacing.md\n@@ -1,1 +1,1 @@\n-tool rows drift apart after inline diffs in compact transcript layouts\n+tool rows stay packed tightly after inline diffs in compact transcript layouts\n",
    )
    .expect("write compact diff fixture");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry = transcript_section_model_test_activity(
        "request-inline-diff-compact-tools",
        ActivityStatus::Done,
        "",
    );
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-read-before-diff".to_string(),
        tool_id: "fs.read".to_string(),
        canonical_tool_id: Some("fs.read".to_string()),
        alias_source_tool_id: Some("read".to_string()),
        resolved_tool_identity: None,
        args_summary: r#"{"path":"docs/spacing.md"}"#.to_string(),
        args_digest: "digest-read-before-diff".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("12 lines read".to_string()),
        output_digest: Some("digest-read-before-diff-output".to_string()),
        output_json: None,
        truncated_output: Some("12 lines read".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 1,
        last_seq: 2,
        first_mono_ms: 1,
        last_mono_ms: 2,
        first_timestamp: None,
        last_timestamp: None,
    });
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-apply-patch-compact".to_string(),
        tool_id: "apply_patch".to_string(),
        canonical_tool_id: Some("apply_patch".to_string()),
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"patchText":"*** Begin Patch"}"#.to_string(),
        args_digest: "digest-apply-patch-compact".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Success. Updated the following files".to_string()),
        output_digest: Some("digest-apply-patch-compact-output".to_string()),
        output_json: Some(serde_json::json!({
            "files": ["M docs/spacing.md"],
            "edits": [
                {
                    "edit_id": "apply-patch-compact",
                    "path": "docs/spacing.md",
                    "summary": "apply patch update docs/spacing.md",
                    "deleted": false,
                    "diff_rel_path": "artifacts/apply-compact.diff",
                    "diff_digest": "digest-apply-compact"
                }
            ]
        })),
        truncated_output: Some("Success. Updated the following files".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: vec![crate::app::ToolArtifactEntry {
            path: "artifacts/apply-compact.diff".to_string(),
            digest: Some("digest-apply-compact".to_string()),
        }],
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 3,
        last_seq: 4,
        first_mono_ms: 3,
        last_mono_ms: 4,
        first_timestamp: None,
        last_timestamp: None,
    });
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-read-after-diff".to_string(),
        tool_id: "fs.read".to_string(),
        canonical_tool_id: Some("fs.read".to_string()),
        alias_source_tool_id: Some("read".to_string()),
        resolved_tool_identity: None,
        args_summary: r#"{"path":"docs/spacing.md"}"#.to_string(),
        args_digest: "digest-read-after-diff".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("12 lines read".to_string()),
        output_digest: Some("digest-read-after-diff-output".to_string()),
        output_json: None,
        truncated_output: Some("12 lines read".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 5,
        last_seq: 6,
        first_mono_ms: 5,
        last_mono_ms: 6,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.set_patch_file_output_expanded_for_test(
        "call-apply-patch-compact",
        "docs/spacing.md",
        true,
    );
    assert!(app.patch_file_output_expanded("call-apply-patch-compact", "docs/spacing.md"));

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));
    let rendered = lines.join("\n");
    let read_before = lines
        .iter()
        .position(|line| line.contains("Read docs/spacing.md"))
        .expect("read-before row");
    let patch_header = lines
        .iter()
        .position(|line| line.contains("Patch · spacing.md"))
        .expect("patch header row");
    let diff_tail = lines
        .iter()
        .rposition(|line| line.contains("compact transcript layouts"))
        .expect("diff tail row");
    let read_after = lines
        .iter()
        .rposition(|line| line.contains("Read docs/spacing.md"))
        .expect("read-after row");

    assert!(read_before < patch_header && patch_header < diff_tail && diff_tail < read_after);
    assert!(
        lines[read_before + 1..patch_header]
            .iter()
            .filter(|line| line.trim().is_empty() || line.trim() == "┃")
            .count()
            <= 2,
        "tool-to-diff spacing should stay compact\n{rendered}"
    );
    assert!(
        lines[diff_tail + 1..read_after]
            .iter()
            .filter(|line| line.trim().is_empty() || line.trim() == "┃")
            .count()
            <= 1,
        "diff-to-tool spacing should stay compact\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_applied_edit_missing_diff_surfaces_fallback() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry = transcript_section_model_test_activity(
        "request-edit-missing-diff",
        ActivityStatus::Done,
        "",
    );
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-edit-missing-diff".to_string(),
        tool_id: "edit.hashline_apply".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"path":"docs/rust.md"}"#.to_string(),
        args_digest: "digest-edit-missing-diff".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Edit applied".to_string()),
        output_digest: Some("digest-edit-missing-output".to_string()),
        output_json: None,
        truncated_output: Some("Edit applied".to_string()),
        edit: Some(crate::app::EditEntry {
            edit_id: "edit-missing-diff-1".to_string(),
            path: "docs/rust.md".to_string(),
            status: crate::app::EditDisplayStatus::Applied,
            summary: Some("Remove the ownership section".to_string()),
            patch_digest: Some("digest-patch".to_string()),
            new_file_digest: Some("digest-new-file".to_string()),
            diff_rel_path: Some("artifacts/missing-edit.diff".to_string()),
            diff_digest: Some("digest-diff".to_string()),
            rejection_reason: None,
        }),
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 140)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("← Patch · rust.md"));
    assert!(rendered.contains("docs"));
    assert!(rendered.contains("Diff preview unavailable"));
    assert!(rendered.contains("artifacts/missing-edit.diff"));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_proposed_edit_renders_header() {
    let mut app = AppState::default();
    let mut entry =
        transcript_section_model_test_activity("request-edit-proposed", ActivityStatus::Done, "");
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-edit-proposed".to_string(),
        tool_id: "edit.hashline_apply".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"path":"crates/harness-tui/src/ui.rs"}"#.to_string(),
        args_digest: "digest-edit-proposed".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Running,
        output_summary: None,
        output_digest: None,
        output_json: None,
        truncated_output: None,
        edit: Some(crate::app::EditEntry {
            edit_id: "edit-proposed-1".to_string(),
            path: "crates/harness-tui/src/ui.rs".to_string(),
            status: crate::app::EditDisplayStatus::Proposed,
            summary: Some("Remove diff review surface".to_string()),
            patch_digest: Some("digest-patch".to_string()),
            new_file_digest: None,
            diff_rel_path: None,
            diff_digest: None,
            rejection_reason: None,
        }),
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 120)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("← Edit · ui.rs"));
    assert!(rendered.contains("crates/harness-tui/src"));
    assert!(!rendered.contains("tool edit.hashline_apply"));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_harness_tool_progress_indicators() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-harness-tool-progress",
        ActivityStatus::Done,
        "",
    );

    let mut pending_read = transcript_section_model_test_tool_call("call-read-pending", "fs.read");
    pending_read.status = ToolCallDisplayStatus::Queued;

    let mut running_read = transcript_section_model_test_tool_call("call-read-running", "fs.read");
    running_read.args_summary = r#"{"path":"src/lib.rs","offset":3,"limit":8}"#.to_string();
    running_read.status = ToolCallDisplayStatus::Running;

    let mut pending_edit = transcript_section_model_test_tool_call("call-edit-pending", "edit");
    pending_edit.status = ToolCallDisplayStatus::Queued;

    let mut pending_patch =
        transcript_section_model_test_tool_call("call-patch-pending", "apply_patch");
    pending_patch.status = ToolCallDisplayStatus::Queued;

    entry.tool_calls = vec![pending_read, running_read, pending_edit, pending_patch];
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let initial_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        120,
    ));
    let initial_rendered = initial_lines.join("\n");
    assert!(
        initial_rendered.contains("~ Reading file..."),
        "missing pending read indicator\n{initial_rendered}"
    );
    assert!(initial_lines
        .iter()
        .any(|line| line.contains("⠋ Read src/lib.rs [offset=3, limit=8]")));
    assert!(initial_lines
        .iter()
        .any(|line| line.contains("~ Preparing edit...")));
    assert!(initial_lines
        .iter()
        .any(|line| line.contains("~ Preparing patch...")));

    app.advance_transcript_animation_phase();

    let updated_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        120,
    ));
    assert!(updated_lines
        .iter()
        .any(|line| line.contains("⠙ Read src/lib.rs [offset=3, limit=8]")));

    let mut mixed_context_app = AppState::default();
    let mut mixed_context_entry = transcript_section_model_test_activity(
        "request-mixed-context-progress",
        ActivityStatus::Done,
        "",
    );
    let mut completed_read = transcript_section_model_test_tool_call("call-read-done", "fs.read");
    completed_read.args_summary = r#"{"path":"src/lib.rs"}"#.to_string();
    completed_read.status = ToolCallDisplayStatus::Succeeded;
    completed_read.output_summary = Some("12 lines read from src/lib.rs".to_string());

    let mut running_glob = transcript_section_model_test_tool_call("call-glob-running", "fs.glob");
    running_glob.args_summary = r#"{"pattern":"*.rs","path":"src"}"#.to_string();
    running_glob.status = ToolCallDisplayStatus::Running;

    mixed_context_entry.tool_calls = vec![completed_read, running_glob];
    mixed_context_app.activities = std::collections::VecDeque::from(vec![mixed_context_entry]);

    let mixed_context_rendered = transcript_test_line_texts(build_transcript_lines_for_width(
        &mixed_context_app,
        &Theme::default(),
        120,
    ))
    .join("\n");
    assert!(
        !mixed_context_rendered.contains("Gathering context"),
        "active context tools should stay as per-tool indicators\n{mixed_context_rendered}"
    );
    assert!(mixed_context_rendered.contains("→ Read src/lib.rs"));
    assert!(mixed_context_rendered.contains("✱ Glob \"*.rs\" in src"));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_rejected_edit_surfaces_reason_inline() {
    let mut app = AppState::default();
    let mut entry =
        transcript_section_model_test_activity("request-edit-rejected", ActivityStatus::Error, "");
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-edit-rejected".to_string(),
        tool_id: "edit.hashline_apply".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"path":"crates/harness-tui/src/ui.rs"}"#.to_string(),
        args_digest: "digest-edit-rejected".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Failed,
        output_summary: Some("ANCHOR_MISMATCH".to_string()),
        output_digest: None,
        output_json: None,
        truncated_output: Some("ANCHOR_MISMATCH".to_string()),
        edit: Some(crate::app::EditEntry {
            edit_id: "edit-rejected-1".to_string(),
            path: "crates/harness-tui/src/ui.rs".to_string(),
            status: crate::app::EditDisplayStatus::Rejected,
            summary: Some("Remove diff review surface".to_string()),
            patch_digest: Some("digest-patch".to_string()),
            new_file_digest: None,
            diff_rel_path: None,
            diff_digest: None,
            rejection_reason: Some("ANCHOR_MISMATCH at line 45".to_string()),
        }),
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 120)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("← Edit · ui.rs"));
    assert!(rendered.contains("crates/harness-tui/src"));
    assert!(rendered.contains("ANCHOR_MISMATCH at line 45"));
}

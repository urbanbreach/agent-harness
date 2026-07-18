use super::super::*;
use crate::UnwrapOrAbort;

#[cfg(test)]
pub(crate) fn exact_test_transcript_edit_tool_matches_inline_diff_shape() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).unwrap_or_abort();
    std::fs::write(
        artifacts_dir.join("harness-inline.diff"),
        "--- crates/harness-tui/src/ui.rs\n+++ crates/harness-tui/src/ui.rs\n@@ -44,8 +44,7 @@\n use ui_secondary::{\n-    render_diff_tab, render_events_tab, render_help_tab,\n+    render_events_tab, render_help_tab, render_live_details_overlay,\n     render_operator_sidebar,\n };\n",
    )
    .unwrap_or_abort();

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

    assert!(rendered.contains("Patch · ui.rs") || rendered.contains("◆ Patch"));
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
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).unwrap_or_abort();
    std::fs::write(
        artifacts_dir.join("native-edit.diff"),
        "--- docs/rust.md\n+++ docs/rust.md\n@@ -1,3 +1,2 @@\n # Rust\n-## Ownership\n Safe and fast\n",
    )
    .unwrap_or_abort();

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

    assert!(rendered.contains("Edit docs/rust.md") || rendered.contains("◆ Edit"));
    assert!(rendered.contains("docs"));
    assert!(rendered.contains("Ownership"));
    assert!(rendered
        .lines()
        .any(|line| line.contains("Ownership") && line.contains('-')));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_apply_patch_multifile_uses_output_edit_paths() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).unwrap_or_abort();
    std::fs::write(
        artifacts_dir.join("apply-a.diff"),
        "@@ -1,1 +1,1 @@\n-old a\n+new a\n",
    )
    .unwrap_or_abort();
    std::fs::write(
        artifacts_dir.join("apply-b.diff"),
        "@@ -1,1 +1,1 @@\n-old b\n+new b\n",
    )
    .unwrap_or_abort();

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

    assert!(rendered.contains("Patch 2 files") || rendered.contains("◆ Patch"));
    assert!(!rendered.contains("Patch 2 files  ▸"));
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
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).unwrap_or_abort();
    std::fs::write(
        artifacts_dir.join("apply-rename.diff"),
        "--- src/session_turn.rs\n+++ src/session_diff.rs\n@@ -1,1 +1,1 @@\n-pub fn render_session_turn_diff() {}\n+pub fn render_session_diff_view() {}\n",
    )
    .unwrap_or_abort();
    std::fs::write(
        artifacts_dir.join("apply-wrap.diff"),
        "--- docs/transcript.md\n+++ docs/transcript.md\n@@ -1,1 +1,1 @@\n-session turn diff view keeps the tool row spacing perfectly aligned in every transcript lane for operators reviewing compact windows\n+session turn diff view keeps the tool row spacing perfectly aligned across the transcript surface for operators reviewing compact windows and narrow shells\n",
    )
    .unwrap_or_abort();

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
        .unwrap_or_abort();
    assert!(
        rename_header.contains("session_diff.rs"),
        "rename header: {rename_header}"
    );
    assert!(rendered.contains("Patch 2 files") || rendered.contains("◆ Patch"));
    assert!(!rendered.contains("Patch 2 files  ▸"));
    assert!(
        lines.iter().any(|line| {
            line.contains("session turn diff view keeps the tool row spacing perfectly align")
        }),
        "long diff prefix missing\n{rendered}"
    );
    assert!(
        lines.iter().any(|line| {
            line.contains("every transcript lane for operators reviewing compact windows")
                || line.contains("n every transcript lane for operators reviewing compact windows")
        }),
        "removed line continuation missing\n{rendered}"
    );
    assert!(
        lines.iter().any(|line| {
            line.contains("across the transcript surface for operators reviewing compact wind")
                || line.contains("ross the transcript surface for operators reviewing compact wind")
        }),
        "added line wrapped continuation prefix missing\n{rendered}"
    );
    assert!(
        lines.iter().any(|line| {
            line.contains("narrow shells")
        }),
        "added line wrapped continuation tail missing\n{rendered}"
    );
    assert!(
        !rendered.contains('…'),
        "wrapped transcript diff should not fall back to truncation\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_inline_diff_stays_compact_between_tool_rows() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).unwrap_or_abort();
    std::fs::write(
        artifacts_dir.join("apply-compact.diff"),
        "--- docs/spacing.md\n+++ docs/spacing.md\n@@ -1,1 +1,1 @@\n-tool rows drift apart after inline diffs in compact transcript layouts\n+tool rows stay packed tightly after inline diffs in compact transcript layouts\n",
    )
    .unwrap_or_abort();

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
        .unwrap_or_abort();
    let patch_header = lines
        .iter()
        .position(|line| line.contains("Patch 1 file"))
        .unwrap_or_abort();
    let diff_tail = lines
        .iter()
        .rposition(|line| line.contains("compact transcript layouts"))
        .unwrap_or_abort();
    let read_after = lines
        .iter()
        .rposition(|line| line.contains("Read docs/spacing.md"))
        .unwrap_or_abort();

    assert!(read_before < patch_header && patch_header < diff_tail && diff_tail < read_after);
    assert!(
        lines[read_before + 1..patch_header]
            .iter()
            .filter(|line| line.trim().is_empty())
            .count()
            <= 1,
        "tool-to-diff spacing should stay compact\n{rendered}"
    );
    assert!(
        lines[diff_tail + 1..read_after]
            .iter()
            .filter(|line| line.trim().is_empty())
            .count()
            <= 1,
        "diff-to-tool spacing should stay compact\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_applied_edit_missing_diff_surfaces_fallback() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
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

    assert!(rendered.contains("Patch · rust.md") || rendered.contains("◆ Patch"));
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

    assert!(rendered.contains("Edit · ui.rs") || rendered.contains("◆ Edit"));
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
        initial_rendered.contains("Reading file") || initial_rendered.contains("◆ Reading"),
        "missing pending read indicator\n{initial_rendered}"
    );
    assert!(initial_lines
        .iter()
        .any(|line| line.contains("Read src/lib.rs") || line.contains("Reading file")));
    assert!(initial_lines.iter().any(|line| line.contains("Edit")));
    assert!(initial_lines.iter().any(|line| line.contains("Patch")));

    app.advance_transcript_animation_phase();

    let updated_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        120,
    ));
    assert!(updated_lines
        .iter()
        .any(|line| line.contains("Read src/lib.rs [offset=3, limit=8]")));

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
    assert!(
        mixed_context_rendered.contains("◆ Read 1 file")
            || mixed_context_rendered.contains("Read 1 file"),
        "completed reads use freeze count form\n{mixed_context_rendered}"
    );
    assert!(mixed_context_rendered.contains("◆ Glob") || mixed_context_rendered.contains("Glob \"*.rs\""));
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

    assert!(rendered.contains("Edit · ui.rs") || rendered.contains("◆ Edit"));
    assert!(rendered.contains("crates/harness-tui/src"));
    assert!(rendered.contains("ANCHOR_MISMATCH at line 45"));
}


#[cfg(test)]
pub(crate) fn exact_test_write_tool_hides_redundant_patched_file_header() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let mut write = transcript_section_model_test_tool_call("call-write-creating", "fs.write");
    write.args_summary = r#"{"path":"demo.txt","content":"parity-diff-ok\n"}"#.to_string();
    write.status = ToolCallDisplayStatus::Running;
    write.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Running);

    let section = build_transcript_tool_call_section(
        &write,
        &AppState::default(),
        None,
        true,
        false,
        true,
        false,
        Some(run_dir.path()),
    );

    assert_eq!(section.header.title, "Creating demo.txt");
    let structured = section
        .detail_blocks
        .iter()
        .filter_map(|block| match block {
            TranscriptToolCallDetailBlock::StructuredDiff {
                diff_content,
                show_file_header,
                force_stacked,
                ..
            } => Some((diff_content.as_str(), *show_file_header, *force_stacked)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        !structured.is_empty(),
        "write tools must keep inline diff body blocks\n{section:#?}"
    );
    assert!(
        structured.iter().all(|(_, show_header, _)| !*show_header),
        "single-file write title already carries path; hide redundant ← Patched header\n{structured:#?}"
    );
    assert!(
        structured.iter().all(|(_, _, force_stacked)| *force_stacked),
        "write create diffs must force stacked packing at wide geometry\n{structured:#?}"
    );
    assert!(
        structured
            .iter()
            .any(|(diff, _, _)| diff.contains("parity-diff-ok")),
        "write body diff content must still be present\n{structured:#?}"
    );

    let rendered = append_tool_call_section_lines(&section, &Theme::default(), 120, Theme::default().surface.panel)
        .lines
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !rendered.contains("← Patched"),
        "rendered write row must not surface ← Patched header\n{rendered}"
    );
    assert!(
        rendered.contains("parity-diff-ok"),
        "rendered write row must keep diff body\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_write_tool_renders_plain_numbered_dual_line_body() {
    // Given: a write overwrite with before+after content (Grok Creating dual-line form)
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let mut write = transcript_section_model_test_tool_call("call-write-dual", "fs.write");
    write.args_summary =
        r#"{"path":"demo.txt","content":"parity-diff-ok\n","oldContent":"old content\n"}"#
            .to_string();
    write.status = ToolCallDisplayStatus::Running;
    write.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Running);

    // When: building and rendering the write tool section
    let section = build_transcript_tool_call_section(
        &write,
        &AppState::default(),
        None,
        true,
        false,
        true,
        false,
        Some(run_dir.path()),
    );
    let rendered = append_tool_call_section_lines(
        &section,
        &Theme::default(),
        120,
        Theme::default().surface.panel,
    )
    .lines
    .into_iter()
    .map(|line| {
        line.spans
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>()
    })
    .collect::<Vec<_>>()
    .join("\n");

    // Then: dual-line plain numbered body without unified-diff +/- markers
    assert!(
        rendered.contains("old content"),
        "write overwrite body must surface before content\n{rendered}"
    );
    assert!(
        rendered.contains("parity-diff-ok"),
        "write overwrite body must surface after content\n{rendered}"
    );
    let body_lines: Vec<_> = rendered
        .lines()
        .filter(|line| line.contains("old content") || line.contains("parity-diff-ok"))
        .collect();
    assert_eq!(
        body_lines.len(),
        2,
        "write overwrite body must render both before and after lines\n{rendered}"
    );
    assert!(
        body_lines.iter().all(|line| {
            let trimmed = line.trim_start();
            // plain: "1  text" — reject unified marker forms "1 + text" / "1 - text"
            trimmed.starts_with('1')
                && !trimmed.starts_with("1 +")
                && !trimmed.starts_with("1 -")
                && !line.contains("← Patched")
        }),
        "Grok write body is plain numbered dual-line (1  text), not unified +/- markers\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_write_tool_title_matches_thought_lead() {
    // Given: a write tool with rendered plain-numbered body (Block visual path)
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let mut write = transcript_section_model_test_tool_call("call-write-lead", "fs.write");
    write.args_summary =
        r#"{"path":"demo.txt","content":"parity-diff-ok\n","oldContent":"old content\n"}"#
            .to_string();
    write.status = ToolCallDisplayStatus::Running;
    write.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Running);

    // When: rendering the write section lines
    let section = build_transcript_tool_call_section(
        &write,
        &AppState::default(),
        None,
        true,
        false,
        true,
        false,
        Some(run_dir.path()),
    );
    assert_eq!(section.header.visual_style, TranscriptToolCallVisualStyle::Block);
    let lines: Vec<String> = append_tool_call_section_lines(
        &section,
        &Theme::default(),
        120,
        Theme::default().surface.panel,
    )
    .lines
    .into_iter()
    .map(|line| {
        line.spans
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>()
    })
    .collect();

    let title = lines
        .iter()
        .find(|line| line.contains("Creating demo.txt"))
        .unwrap_or_else(|| panic!("missing Creating title\n{}", lines.join("\n")));
    let body = lines
        .iter()
        .find(|line| line.contains("old content"))
        .unwrap_or_else(|| panic!("missing body line\n{}", lines.join("\n")));

    let title_lead = title.len() - title.trim_start().len();
    let body_lead = body.len() - body.trim_start().len();

    // Then: Creating title is flat (same lead as Thought / inline tools) — no nested
    // invisible rail padding. Body keeps the plain-numbered indent under the title.
    assert_eq!(
        title_lead, 0,
        "Grok Creating title aligns with Thought (flat lead); nested card rail adds +2\n{title:?}\n{}",
        lines.join("\n")
    );
    assert_eq!(
        body_lead, 2,
        "Grok plain body is title+2 (`  1  text`); min-4 line pad was lead=5\ntitle_lead={title_lead} body_lead={body_lead}\n{}",
        lines.join("\n")
    );
}

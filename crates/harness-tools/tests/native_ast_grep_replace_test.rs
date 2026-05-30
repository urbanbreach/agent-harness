use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

mod common;

use common::{setup_workspace_fixture, test_context};
use harness_core::config::ShellAllowlist;
use harness_core::tool::ToolCapability;
use harness_tools::coordinator_registry_with_ast_grep_command;
use serde_json::json;

fn install_fake_ast_grep(workspace: &Path) -> PathBuf {
    let script = workspace.join("fake-ast-grep-replace.py");
    fs::write(
        &script,
        r###"#!/usr/bin/env python3
import json
import pathlib
import sys

args = sys.argv[1:]
pattern = args[args.index("--pattern") + 1] if "--pattern" in args else ""
rewrite = args[args.index("--rewrite") + 1] if "--rewrite" in args else ""
roots = [arg for arg in args if arg.endswith(".rs")]

if pattern == "fn (":
    print("Pattern contains an ERROR node", file=sys.stderr)
    print("[]")
    sys.exit(2)

if pattern.startswith("struct"):
    print("[]")
    sys.exit(1)

matches = []
for root in roots:
    path = pathlib.Path(root)
    source = path.read_text()
    if path.name == "large.rs":
        needle_prefix = "fn item_"
        cursor = 0
        index = 0
        while True:
            start = source.find(needle_prefix, cursor)
            if start < 0:
                break
            line_end = source.find("\n", start)
            if line_end < 0:
                line_end = len(source)
            text = source[start:line_end]
            replacement_end = start + len("fn item_000")
            replacement = source[start:replacement_end].replace("item", "thing")
            matches.append({
                "text": text,
                "range": {
                    "byteOffset": {"start": start, "end": line_end},
                    "start": {"line": index, "column": 0},
                    "end": {"line": index, "column": len(text)},
                },
                "replacement": replacement,
                "replacementOffsets": {"start": start, "end": replacement_end},
                "file": root,
                "language": "Rust",
            })
            cursor = start + len(text)
            index += 1
        continue

    needle = "fn hello_world"
    start = source.find(needle)
    if start >= 0:
        match_end = source.find("\n}", start)
        if match_end < 0:
            match_end = len(source)
        else:
            match_end += len("\n}")
        matches.append({
            "text": source[start:match_end],
            "range": {
                "byteOffset": {"start": start, "end": match_end},
                "start": {"line": 0, "column": start},
                "end": {"line": 2, "column": 1},
            },
            "replacement": rewrite or "fn goodbye_world",
            "replacementOffsets": {"start": start, "end": start + len(needle)},
            "file": root,
            "lines": needle,
            "language": "Rust",
        })

print(json.dumps(matches))
"###,
    )
    .expect("fake ast-grep script");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script)
            .expect("fake ast-grep metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("fake ast-grep executable bit");
    }
    script
}

#[tokio::test]
async fn ast_grep_replace_defaults_to_dry_run_and_apply_uses_safe_edit_artifacts() {
    // arrange
    let workspace = setup_workspace_fixture();
    let fake_ast_grep = install_fake_ast_grep(workspace.workspace());
    fs::create_dir_all(workspace.workspace().join("src")).expect("src dir");
    let source_path = workspace.workspace().join("src/lib.rs");
    fs::write(
        &source_path,
        "fn hello_world() {\n    println!(\"hello\");\n}\n",
    )
    .expect("write rust source");

    let registry = coordinator_registry_with_ast_grep_command(
        ShellAllowlist::default(),
        fake_ast_grep.display().to_string(),
    );
    let tool = registry.get("ast_grep_replace").expect("ast_grep_replace");
    assert_eq!(tool.capability(), ToolCapability::EditFs);
    let ctx = test_context(
        workspace.workspace(),
        "run-ast-grep-replace-test",
        "ast-grep-replace-test",
    );

    // act: default mode is dry_run.
    let dry_run = tool
        .call(
            ctx.clone(),
            json!({
                "language": "rust",
                "pattern": "fn $NAME",
                "rewrite": "fn goodbye_world",
                "paths": ["src/lib.rs"]
            }),
        )
        .await
        .expect("dry-run ast_grep_replace");

    // assert: dry_run produces a diff artifact and does not mutate the file.
    assert_eq!(
        fs::read_to_string(&source_path).expect("read source"),
        "fn hello_world() {\n    println!(\"hello\");\n}\n"
    );
    assert!(
        !dry_run.artifacts.is_empty(),
        "dry-run should write a diff artifact"
    );
    let dry_json = dry_run.structured_json.as_ref().expect("dry-run json");
    assert_eq!(dry_json["mode"], "dry_run");
    assert_eq!(dry_json["applied"], false);
    assert_eq!(dry_json["returned_count"], 1);
    assert_eq!(dry_json["files"][0]["file_path"], "src/lib.rs");
    assert_eq!(
        dry_json["edits"][0]["byte_range"],
        json!({"start": 0, "end": 14})
    );
    assert_eq!(
        dry_json["edits"][0]["match_byte_range"],
        json!({"start": 0, "end": 43})
    );
    assert_eq!(dry_json["edits"][0]["replacement"], "fn goodbye_world");
    assert_eq!(
        dry_json["adapter"]["command"],
        format!("{} run", fake_ast_grep.display())
    );

    // act: apply mode writes through the edit-capability tool and records artifacts.
    let applied = tool
        .call(
            ctx,
            json!({
                "language": "rust",
                "pattern": "fn $NAME",
                "rewrite": "fn goodbye_world",
                "paths": ["src/lib.rs"],
                "mode": "apply"
            }),
        )
        .await
        .expect("apply ast_grep_replace");

    // assert
    assert_eq!(
        fs::read_to_string(&source_path).expect("read applied source"),
        "fn goodbye_world() {\n    println!(\"hello\");\n}\n"
    );
    assert!(
        !applied.artifacts.is_empty(),
        "apply should write a diff artifact"
    );
    let applied_json = applied.structured_json.as_ref().expect("apply json");
    assert_eq!(applied_json["mode"], "apply");
    assert_eq!(applied_json["applied"], true);
}

#[tokio::test]
async fn ast_grep_replace_rejects_unsafe_or_unsupported_requests() {
    // arrange
    let workspace = setup_workspace_fixture();
    let fake_ast_grep = install_fake_ast_grep(workspace.workspace());
    fs::create_dir_all(workspace.workspace().join("src")).expect("src dir");
    fs::write(
        workspace.workspace().join("src/lib.rs"),
        "fn hello_world() {}\n",
    )
    .expect("write rust source");
    let registry = coordinator_registry_with_ast_grep_command(
        ShellAllowlist::default(),
        fake_ast_grep.display().to_string(),
    );
    let tool = registry.get("ast_grep_replace").expect("ast_grep_replace");
    let ctx = test_context(
        workspace.workspace(),
        "run-ast-grep-replace-reject-test",
        "ast-grep-replace-reject-test",
    );

    // act
    let traversal = tool
        .call(
            ctx.clone(),
            json!({
                "language": "rust",
                "pattern": "fn $NAME",
                "rewrite": "fn goodbye_world",
                "paths": ["../outside.rs"]
            }),
        )
        .await
        .expect_err("traversal should be rejected");

    // assert: traversal is rejected before adapter execution.
    assert!(traversal.to_string().contains("parent traversal"));

    let schema_err = tool
        .call(
            ctx.clone(),
            json!({
                "language": "rust",
                "pattern": "fn $NAME",
                "rewrite": "fn goodbye_world",
                "paths": ["src/lib.rs"],
                "unexpected": true
            }),
        )
        .await
        .expect_err("unknown fields should be rejected");
    assert!(schema_err.to_string().contains("unknown field"));

    let unsupported_language = tool
        .call(
            ctx.clone(),
            json!({
                "language": "brainfuck",
                "pattern": "fn $NAME",
                "rewrite": "fn goodbye_world",
                "paths": ["src/lib.rs"]
            }),
        )
        .await
        .expect_err("unsupported language should fail");
    assert!(unsupported_language
        .to_string()
        .contains("unsupported language"));

    let invalid_pattern = tool
        .call(
            ctx.clone(),
            json!({
                "language": "rust",
                "pattern": "fn (",
                "rewrite": "fn goodbye_world",
                "paths": ["src/lib.rs"]
            }),
        )
        .await
        .expect_err("invalid ast-grep pattern should fail");
    assert!(
        invalid_pattern
            .to_string()
            .contains("could not parse pattern"),
        "unexpected invalid pattern error: {invalid_pattern}"
    );

    let no_match = tool
        .call(
            ctx,
            json!({
                "language": "rust",
                "pattern": "struct $NAME",
                "rewrite": "struct Replacement",
                "paths": ["src/lib.rs"]
            }),
        )
        .await
        .expect("no match is successful");
    assert_eq!(
        no_match.structured_json.as_ref().expect("json")["returned_count"],
        0
    );
}

#[tokio::test]
async fn ast_grep_replace_caps_large_dry_runs_and_refuses_partial_apply() {
    // arrange
    let workspace = setup_workspace_fixture();
    let fake_ast_grep = install_fake_ast_grep(workspace.workspace());
    fs::create_dir_all(workspace.workspace().join("src")).expect("src dir");
    let large_source = (0..20)
        .map(|index| format!("fn item_{index:03}() {{}}"))
        .collect::<Vec<_>>()
        .join("\n");
    let large_path = workspace.workspace().join("src/large.rs");
    fs::write(&large_path, large_source.clone()).expect("write large rust source");
    let registry = coordinator_registry_with_ast_grep_command(
        ShellAllowlist::default(),
        fake_ast_grep.display().to_string(),
    );
    let tool = registry.get("ast_grep_replace").expect("ast_grep_replace");
    let ctx = test_context(
        workspace.workspace(),
        "run-ast-grep-replace-large-test",
        "ast-grep-replace-large-test",
    );

    // act
    let dry_run = tool
        .call(
            ctx.clone(),
            json!({
                "language": "rust",
                "pattern": "fn $NAME",
                "rewrite": "fn thing_$NAME",
                "paths": ["src/large.rs"],
                "limit": 5
            }),
        )
        .await
        .expect("large dry-run should cap");

    // assert: dry-run caps the returned edits and records a diff artifact.
    let dry_json = dry_run.structured_json.as_ref().expect("dry json");
    assert_eq!(dry_json["total_count"], 20);
    assert_eq!(dry_json["returned_count"], 5);
    assert_eq!(dry_json["truncated"], true);
    assert!(!dry_run.artifacts.is_empty());
    assert_eq!(
        fs::read_to_string(&large_path).expect("read large"),
        large_source
    );

    let partial_apply = tool
        .call(
            ctx,
            json!({
                "language": "rust",
                "pattern": "fn $NAME",
                "rewrite": "fn thing_$NAME",
                "paths": ["src/large.rs"],
                "limit": 5,
                "mode": "apply"
            }),
        )
        .await
        .expect_err("apply must not partially mutate truncated result sets");
    assert!(partial_apply
        .to_string()
        .contains("refused to apply 20 replacement"));
    assert_eq!(
        fs::read_to_string(&large_path).expect("read large"),
        large_source
    );
}

#[tokio::test]
async fn ast_grep_replace_reports_missing_adapter_actionably() {
    // arrange
    let workspace = setup_workspace_fixture();
    let missing_ast_grep = workspace.workspace().join("missing-ast-grep");
    fs::create_dir_all(workspace.workspace().join("src")).expect("src dir");
    fs::write(
        workspace.workspace().join("src/lib.rs"),
        "fn missing_adapter() {}\n",
    )
    .expect("write rust source");

    let registry = coordinator_registry_with_ast_grep_command(
        ShellAllowlist::default(),
        missing_ast_grep.display().to_string(),
    );
    let ctx = test_context(
        workspace.workspace(),
        "run-ast-grep-replace-missing-test",
        "ast-grep-replace-missing-test",
    );

    // act
    let err = registry
        .get("ast_grep_replace")
        .expect("ast_grep_replace")
        .call(
            ctx,
            json!({
                "language": "rust",
                "pattern": "fn $NAME",
                "rewrite": "fn renamed",
                "paths": ["src/lib.rs"]
            }),
        )
        .await
        .expect_err("missing ast-grep adapter should fail actionably");

    // assert
    assert!(
        err.to_string()
            .contains("requires the `ast-grep` binary on PATH"),
        "unexpected missing adapter error: {err}"
    );
}

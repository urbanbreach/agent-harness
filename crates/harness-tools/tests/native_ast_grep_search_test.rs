use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

mod common;

use common::{setup_workspace_fixture, test_context};
use harness_core::config::ShellAllowlist;
use harness_tools::coordinator_registry_with_ast_grep_command;
use serde_json::json;

fn install_fake_ast_grep(workspace: &Path) -> PathBuf {
    let script = workspace.join("fake-ast-grep.py");
    fs::write(
        &script,
        r###"#!/usr/bin/env python3
import json
import sys

args = sys.argv[1:]
pattern = args[args.index("--pattern") + 1] if "--pattern" in args else ""

if pattern == "fn (":
    print("Pattern contains an ERROR node", file=sys.stderr)
    print("[]")
    sys.exit(2)

if pattern.startswith("struct"):
    print("[]")
    sys.exit(1)

is_large = any(arg.endswith("src/large.rs") for arg in args)
count = 1200 if is_large else 1
file_path = "src/large.rs" if is_large else "src/lib.rs"
matches = []
for index in range(count):
    text = f"fn generated_match_{index}() {{ println!(\"fake adapter {index}\"); }}" if is_large else "fn hello_world() {\n    println!(\"fake adapter\");\n}"
    matches.append({
        "text": text,
        "range": {
            "byteOffset": {"start": 0, "end": len(text)},
            "start": {"line": index, "column": 0},
            "end": {"line": index, "column": len(text)},
        },
        "file": file_path,
        "lines": text,
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
async fn ast_grep_search_is_read_only_workspace_safe_and_structured() {
    // arrange
    let workspace = setup_workspace_fixture();
    let fake_ast_grep = install_fake_ast_grep(workspace.workspace());
    fs::create_dir_all(workspace.workspace().join("src")).expect("src dir");
    fs::write(
        workspace.workspace().join("src/lib.rs"),
        "fn hello_world() {\n    println!(\"hello\");\n}\n",
    )
    .expect("write rust source");

    let registry = coordinator_registry_with_ast_grep_command(
        ShellAllowlist::default(),
        fake_ast_grep.display().to_string(),
    );
    let ctx = test_context(workspace.workspace(), "run-ast-grep-test", "ast-grep-test");

    // act
    let result = registry
        .get("ast_grep_search")
        .expect("ast_grep_search")
        .call(
            ctx.clone(),
            json!({
                "language": "rust",
                "pattern": "fn $NAME",
                "paths": ["src/lib.rs"],
                "limit": 5
            }),
        )
        .await
        .expect("ast_grep_search");

    // assert
    assert_eq!(
        result
            .structured_json
            .as_ref()
            .and_then(|json| json.get("source"))
            .and_then(serde_json::Value::as_str),
        Some("ast_grep_cli_adapter")
    );
    assert_eq!(
        result
            .structured_json
            .as_ref()
            .and_then(|json| json.pointer("/adapter/command"))
            .and_then(serde_json::Value::as_str),
        Some(format!("{} run", fake_ast_grep.display()).as_str())
    );
    assert_eq!(
        result
            .structured_json
            .as_ref()
            .and_then(|json| json.get("returned_count"))
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    let first_match = result
        .structured_json
        .as_ref()
        .and_then(|json| json.get("matches"))
        .and_then(serde_json::Value::as_array)
        .and_then(|matches| matches.first())
        .expect("ast-grep match");
    assert!(
        first_match
            .get("matched_text")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|text| text.contains("fake adapter")),
        "ast_grep_search should use the deterministic fake adapter in tests"
    );

    let err = registry
        .get("ast_grep_search")
        .expect("ast_grep_search")
        .call(
            ctx.clone(),
            json!({
                "language": "rust",
                "pattern": "fn $NAME",
                "paths": ["../outside.rs"]
            }),
        )
        .await
        .expect_err("traversal should be rejected");
    assert!(err.to_string().contains("parent traversal"));

    let schema_err = registry
        .get("ast_grep_search")
        .expect("ast_grep_search")
        .call(
            ctx.clone(),
            json!({
                "language": "rust",
                "pattern": "fn $NAME",
                "paths": ["src/lib.rs"],
                "unexpected": true
            }),
        )
        .await
        .expect_err("unknown fields should be rejected");
    assert!(schema_err.to_string().contains("unknown field"));

    let unsupported_language = registry
        .get("ast_grep_search")
        .expect("ast_grep_search")
        .call(
            ctx.clone(),
            json!({
                "language": "brainfuck",
                "pattern": "fn $NAME",
                "paths": ["src/lib.rs"]
            }),
        )
        .await
        .expect_err("unsupported language should fail");
    assert!(unsupported_language
        .to_string()
        .contains("unsupported language"));

    let invalid_pattern = registry
        .get("ast_grep_search")
        .expect("ast_grep_search")
        .call(
            ctx.clone(),
            json!({
                "language": "rust",
                "pattern": "fn (",
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

    let inferred_language = registry
        .get("ast_grep_search")
        .expect("ast_grep_search")
        .call(
            ctx.clone(),
            json!({
                "pattern": "fn $NAME",
                "paths": ["src/lib.rs"],
                "limit": 999,
                "context": 999
            }),
        )
        .await
        .expect("single file language inference should work");
    assert_eq!(
        inferred_language
            .structured_json
            .as_ref()
            .and_then(|json| json.get("language_inference"))
            .and_then(serde_json::Value::as_str),
        Some("single_language_from_paths")
    );
    assert_eq!(
        inferred_language
            .structured_json
            .as_ref()
            .and_then(|json| json.get("effective_limit"))
            .and_then(serde_json::Value::as_u64),
        Some(200)
    );
    assert_eq!(
        inferred_language
            .structured_json
            .as_ref()
            .and_then(|json| json.get("effective_context"))
            .and_then(serde_json::Value::as_u64),
        Some(5)
    );

    let no_match = registry
        .get("ast_grep_search")
        .expect("ast_grep_search")
        .call(
            ctx.clone(),
            json!({
                "language": "rust",
                "pattern": "struct $NAME",
                "paths": ["src/lib.rs"]
            }),
        )
        .await
        .expect("no match is successful");
    assert_eq!(
        no_match
            .structured_json
            .as_ref()
            .and_then(|json| json.get("returned_count"))
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );

    let large_source = (0..1200)
        .map(|index| format!("fn generated_match_{index}() {{ println!(\"{index}\"); }}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(workspace.workspace().join("src/large.rs"), large_source)
        .expect("write large rust source");
    let spilled = registry
        .get("ast_grep_search")
        .expect("ast_grep_search")
        .call(
            ctx,
            json!({
                "language": "rust",
                "pattern": "fn $NAME",
                "paths": ["src/large.rs"],
                "limit": 1200
            }),
        )
        .await
        .expect("large ast_grep_search spills");
    assert!(
        !spilled.artifacts.is_empty(),
        "large ast_grep_search output should spill to an artifact"
    );
}

#[tokio::test]
async fn ast_grep_search_reports_missing_adapter_actionably() {
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
        "run-ast-grep-missing-test",
        "ast-grep-missing-test",
    );

    // act
    let err = registry
        .get("ast_grep_search")
        .expect("ast_grep_search")
        .call(
            ctx,
            json!({
                "language": "rust",
                "pattern": "fn $NAME",
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

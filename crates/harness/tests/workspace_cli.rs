use harness::{run, CliDeps, CliIo};
use serde_json::Value;
use std::{fs, io::Cursor};

#[test]
fn memory_commands_keep_reads_empty_and_scope_redacted_writes_to_the_requested_workspace(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let selected = root.path().join("selected");
    fs::create_dir(&selected)?;
    let data = root.path().join("data");
    let runtime = harness_core::storage_paths::ProjectPaths::new(&data, &selected)?.runtime_dir();
    let deps = CliDeps::real()
        .with_current_dir(root.path().into())
        .with_env("HARNESS_HOME", root.path().join("data").to_string_lossy());
    for action in [
        vec!["list"],
        vec!["put", "preference", "Use tabs. api_key=private-value"],
        vec!["get", "preference"],
        vec!["search", "TABS"],
    ] {
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            ["harness", "memory", "--workspace", "selected"]
                .into_iter()
                .chain(action.clone()),
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            deps.clone(),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let text = String::from_utf8(stdout)?;
        assert!(!text.contains("private-value"));
        let json: Value = serde_json::from_str(&text)?;
        if action[0] == "list" {
            assert_eq!(json["entries"].as_array().map(Vec::len), Some(0));
            assert!(!selected.join(".harness").exists());
        } else {
            assert!(text.contains("Use tabs."));
            assert!(text.contains("[REDACTED]"));
        }
        assert!(!root.path().join(".harness").exists());
        assert!(!selected.join(".harness").exists());
    }
    let stored = runtime.join("memory/entries.json");
    assert!(!fs::read_to_string(stored)?.contains("private-value"));
    Ok(())
}

#[test]
fn graph_commands_report_missing_indexes_and_query_the_selected_workspace(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(
        root.path().join("sample.rs"),
        "pub fn entry() {\n    helper();\n}\npub fn helper() {}\n",
    )?;
    let runtime =
        harness_core::storage_paths::ProjectPaths::new(&root.path().join("data"), root.path())?
            .runtime_dir();
    let index = runtime.join(harness_core::code_graph::GRAPH_INDEX_FILE);
    for (args, succeeds) in [
        (vec!["query", "helper"], false),
        (vec!["build"], true),
        (vec!["query", "helper", "--kind", "callers"], true),
    ] {
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            ["harness", "code-graph"].into_iter().chain(args.clone()),
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            CliDeps::real()
                .with_current_dir(root.path().into())
                .with_env("HARNESS_HOME", root.path().join("data").to_string_lossy()),
        );
        assert_eq!(
            result.code == 0,
            succeeds,
            "{}",
            String::from_utf8_lossy(&stderr)
        );
        let report: Value = serde_json::from_slice(&stdout)?;
        if !succeeds {
            assert_eq!(report["result"]["outcome"], "unavailable");
            assert!(!index.exists());
        } else if args[0] == "query" {
            assert_eq!(report["result"]["hits"][0]["symbol"], "entry");
            assert_eq!(report["result"]["hits"][0]["path"], "sample.rs");
        } else {
            assert_eq!(report["symbol_count"], 2);
        }
        assert!(!root.path().join(".harness").exists());
    }
    Ok(())
}

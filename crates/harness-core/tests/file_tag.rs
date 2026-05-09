use harness_core::file_tag::{
    files, materialize_file_tag_context, materialize_file_tag_context_with_selected,
    materialize_prompt_part_context, FileTagLineRange, FileTagSource, SelectedAgentTag,
    SelectedFileTag, SelectedResourceTag,
};

#[test]
fn files_matches_opencode_markdown_file_regex_examples() {
    let template = r#"This is a @valid/path/to/a/file and it should also match at
  the beginning of a line:

  @another-valid/path/to/a/file

  but this is not:

     - Adds a "Co-authored-by:" footer which clarifies which AI agent
       helped create this commit, using an appropriate `noreply@...`
       or `noreply@anthropic.com` email address.

  We also need to deal with files followed by @commas, ones
  with @file-extensions.md, even @multiple.extensions.bak,
  hidden directories like @.config/ or files like @.bashrc
  and ones at the end of a sentence like @foo.md.

  Also shouldn't forget @/absolute/paths.txt with and @/without/extensions,
  as well as @~/home-files and @~/paths/under/home.txt.

  If the reference is `@quoted/in/backticks` then it shouldn't match at all."#;

    let names = files(template)
        .into_iter()
        .map(|file_match| file_match.name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "valid/path/to/a/file",
            "another-valid/path/to/a/file",
            "commas",
            "file-extensions.md",
            "multiple.extensions.bak",
            ".config/",
            ".bashrc",
            "foo.md",
            "/absolute/paths.txt",
            "/without/extensions",
            "~/home-files",
            "~/paths/under/home.txt",
        ]
    );
}

#[test]
fn materialize_file_tag_context_reads_files_and_directories_once() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = tempdir.path();
    std::fs::write(root.join("alpha.txt"), "first\nsecond\n").expect("write file");
    std::fs::create_dir(root.join("src")).expect("create dir");
    std::fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").expect("write nested file");

    let context = materialize_file_tag_context(root, "read @alpha.txt and @src and @alpha.txt")
        .expect("context");

    assert!(context.contains("Called the Read tool with the following input:"));
    assert!(context.contains("alpha.txt"));
    assert!(context.contains("1: first\n2: second"));
    assert!(context.contains("lib.rs"));
    assert_eq!(context.matches("alpha.txt").count(), 1);
}

#[test]
fn materialize_file_tag_context_ignores_missing_paths() {
    let tempdir = tempfile::tempdir().expect("tempdir");

    assert_eq!(
        materialize_file_tag_context(tempdir.path(), "read @missing.txt"),
        None
    );
}

#[test]
fn materialize_file_tag_context_reports_paths_outside_workspace() {
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    let external = tempfile::NamedTempFile::new().expect("external file");

    let context = materialize_file_tag_context(
        workspace.path(),
        &format!("read @{}", external.path().display()),
    )
    .expect("context");

    assert!(context.contains("Read tool failed to read"));
    assert!(context.contains("path escapes workspace root"));
}

#[test]
fn materialize_file_tag_context_honors_line_ranges() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = tempdir.path();
    std::fs::write(root.join("alpha.txt"), "one\ntwo\nthree\nfour\n").expect("write file");

    let context = materialize_file_tag_context(root, "read @alpha.txt#2-3").expect("context");

    assert!(context.contains("2: two\n3: three"));
    assert!(!context.contains("1: one"));
    assert!(!context.contains("4: four"));
}

#[test]
fn selected_file_tags_are_materialized_once_with_structured_metadata() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = tempdir.path();
    std::fs::write(root.join("alpha.txt"), "one\ntwo\nthree\n").expect("write file");

    let selected = SelectedFileTag {
        path: "alpha.txt".to_string(),
        filename: "alpha.txt#2".to_string(),
        url: "file:///workspace/alpha.txt?start=2".to_string(),
        mime: "text/plain".to_string(),
        source: FileTagSource {
            start: 5,
            end: 17,
            value: "@alpha.txt#2".to_string(),
        },
        line_range: Some(FileTagLineRange {
            start: 2,
            end: None,
        }),
    };

    let context =
        materialize_file_tag_context_with_selected(root, "read @alpha.txt#2", &[selected])
            .expect("context");

    assert!(context.contains("2: two"));
    assert!(!context.contains("1: one"));
    assert!(!context.contains("3: three"));
    assert_eq!(context.matches("alpha.txt").count(), 1);
}

#[test]
fn selected_agent_and_resource_tags_materialize_prompt_context() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let context = materialize_prompt_part_context(
        tempdir.path(),
        "ask @plan about @mcp://docs/guide",
        &[],
        &[SelectedAgentTag {
            name: "plan".to_string(),
            source: FileTagSource {
                start: 4,
                end: 9,
                value: "@plan".to_string(),
            },
        }],
        &[SelectedResourceTag {
            name: "Docs Guide".to_string(),
            uri: "mcp://docs/guide".to_string(),
            mime: "text/markdown".to_string(),
            description: Some("Documentation index".to_string()),
            source: FileTagSource {
                start: 16,
                end: 33,
                value: "@mcp://docs/guide".to_string(),
            },
        }],
    )
    .expect("context");

    assert!(context.contains("Selected agent mention: @plan"));
    assert!(context.contains("Use the task tool with subagent `plan`"));
    assert!(context.contains("Selected MCP resource: Docs Guide"));
    assert!(context.contains("URI: mcp://docs/guide"));
    assert!(context.contains("MIME: text/markdown"));
    assert!(context.contains("Description: Documentation index"));
}

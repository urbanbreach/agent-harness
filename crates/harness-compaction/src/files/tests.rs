use super::*;
use crate::prepare::tests::item;
use harness_providers::{AssistantToolCall, CompletionMessage, MessageRole};

#[test]
fn tracking_and_legacy_selector_parity() {
    let selectors = [
        "50",
        "50-",
        "50-200",
        "50+150",
        "5-16,960-973",
        "2724..2727",
        "raw",
        "conflicts",
        "100-170:raw",
        "raw:2-4",
    ];
    let reads: Vec<_> = selectors
        .iter()
        .map(|suffix| format!("src/foo.ts:{suffix}"))
        .collect();
    let mut ops = FileOps::default();
    merge_previous(&mut ops, &reads, &[]);
    assert_eq!(file_lists(&ops), (vec!["src/foo.ts".into()], vec![]));
    let legacy = [
        "archive.zip:dir/file.ts:50-60",
        "archive.zip:dir/file.ts",
        "db.sqlite:users",
        "ordinary",
    ];
    merge_previous(
        &mut ops,
        &legacy.map(str::to_owned),
        &["edit.rs".into(), "local://excluded".into()],
    );
    let mut message = CompletionMessage::text(MessageRole::Assistant, "");
    message.assistant_tool_calls = Some(
        [
            ("read", "filePath", "src/login.ts"),
            ("read", "file_path", "src/login.ts"),
            ("read", "path", "src/login.ts"),
            ("write", "path", "src/login.ts"),
            ("read", "path", "artifact://1"),
            ("read", "path", "HTTPS://host/file"),
            ("write", "path", "src/login.ts:conflict://3"),
            ("edit", "path", "agent://excluded"),
        ]
        .into_iter()
        .map(|(name, key, path)| AssistantToolCall {
            tool_call_id: String::new(),
            function_name: name.into(),
            arguments_json: serde_json::json!({key:path,"offset":30,"limit":50}).to_string(),
        })
        .collect(),
    );
    collect_file_ops(&[item(&message)], &mut ops);
    assert_eq!(
        file_lists(&ops),
        (
            vec![
                "archive.zip:dir/file.ts".into(),
                "db.sqlite:users".into(),
                "ordinary".into(),
                "src/foo.ts".into()
            ],
            vec!["edit.rs".into(), "src/login.ts".into()]
        )
    );
    for (path, excluded) in [
        ("src/login.ts:conflict://3", true),
        ("C:/Users/me/file.ts", false),
        ("db.sqlite:users", false),
    ] {
        assert_eq!(scheme_path(path), excluded);
    }
}

#[test]
fn path_argument_precedence_and_supersede_keys() {
    for (json, expected) in [
        (r#"{"path":"f"}"#, "f"),
        (r#"{"filePath":"f","offset":1}"#, "f"),
        (r#"{"file_path":"f","offset":2,"limit":4}"#, "f\x002-4"),
        (r#"{"path":"f","limit":4}"#, "f\x001-4"),
        (r#"{"path":"f","offset":9,"limit":4}"#, "f\x009-4"),
        (r#"{"path":"f","offset":9}"#, "f\x009-"),
        (r#"{"path":"p","filePath":"a","file_path":"b"}"#, "p"),
        (r#"{"path":"","filePath":"a","file_path":"b"}"#, "a"),
        (r#"{"path":7,"file_path":"b"}"#, "b"),
        (r#"{"filePath":"a","file_path":"b"}"#, "a"),
    ] {
        assert_eq!(read_supersede_key(json), Some(expected.into()), "{json}");
    }
    assert_eq!(read_supersede_key(r#"{"path":"https://h/f"}"#), None);
    let keys: BTreeSet<_> = [
        r#"{"path":"f"}"#,
        r#"{"path":"f","offset":5,"limit":10}"#,
        r#"{"path":"f","offset":50,"limit":10}"#,
    ]
    .iter()
    .filter_map(|json| read_supersede_key(json))
    .collect();
    assert_eq!(keys.len(), 3);
}

#[test]
fn selector_grammar_and_alternate_forms() {
    for (path, base, selector, alternate) in [
        ("src/a:100-170:raw", "src/a", Some("100-170:raw"), true),
        ("src/a:raw:2-4", "src/a", Some("raw:2-4"), true),
        ("src/a:RAW", "src/a", Some("RAW"), true),
        ("src/a:Conflicts", "src/a", Some("Conflicts"), true),
        ("src/a:conflicts:50", "src/a:conflicts", Some("50"), false),
        ("src/a:50", "src/a", Some("50"), false),
        ("src/a:L50-L60", "src/a", Some("L50-L60"), false),
        ("src/a:5-16,960-973", "src/a", Some("5-16,960-973"), false),
        ("src/a:50-", "src/a", Some("50-"), false),
        ("src/a:50..", "src/a", Some("50.."), false),
        ("src/a:50-L", "src/a:50-L", None, false),
        ("src/a:50..L", "src/a:50..L", None, false),
        ("src/a:50+L", "src/a:50+L", None, false),
        ("src/a:50+", "src/a:50+", None, false),
        ("src/a:5 0", "src/a:5 0", None, false),
        (
            "archive.zip:dir/f.ts:50-60",
            "archive.zip:dir/f.ts",
            Some("50-60"),
            false,
        ),
        ("archive.zip:dir/f.ts", "archive.zip:dir/f.ts", None, false),
        (":50", ":50", None, false),
        ("db.sqlite:users", "db.sqlite:users", None, false),
    ] {
        let (stripped, found) = strip_selector(path);
        assert_eq!((stripped, found), (base, selector), "{path}");
        assert_eq!(
            found.is_some_and(is_alternate_selector),
            alternate,
            "{path}"
        );
    }
}

#[test]
fn lists_and_display_selection_use_utf16_order() {
    let (high, bmp) = ("\u{10000}.txt", "\u{ffff}.txt");
    let mut ops = FileOps::default();
    ops.read.extend([bmp.to_owned(), high.to_owned()]);
    ops.edited.extend([bmp.to_owned(), high.to_owned()]);
    assert_eq!(file_lists(&ops).1, vec![high, bmp]);
    ops.edited.clear();
    assert_eq!(file_lists(&ops).0, vec![high, bmp]);
    let mut paths: Vec<String> = (0..19).map(|n| format!("f{n:02}")).collect();
    paths.extend([bmp.to_owned(), high.to_owned()]);
    let rendered = render_files_block(&paths, &[], &BTreeSet::new());
    assert!(
        rendered.contains(high) && !rendered.contains(bmp),
        "{rendered}"
    );
    assert!(rendered.ends_with("[…1 files elided…]\n</files>"));
}

#[test]
fn exact_tree_modes_folding_cap_and_upsert() {
    let read = ["src/a.ts", "src/b.ts"].map(str::to_owned);
    let modified = ["src/c.ts", "src/d.ts"].map(str::to_owned);
    let set = ["src/a.ts", "src/b.ts", "src/c.ts"]
        .map(str::to_owned)
        .into_iter()
        .collect();
    let block = render_files_block(&read, &modified, &set);
    assert_eq!(
        block,
        "<files>\n# src/\na.ts (Read)\nb.ts (Read)\nc.ts (RW)\nd.ts (Write)\n</files>"
    );
    assert_eq!(
        render_files_block(&[], &["c.ts".into()], &BTreeSet::new()),
        "<files>\nc.ts (Write)\n</files>"
    );
    assert_eq!(
        render_files_block(
            &[
                "root".into(),
                "src/deep/a".into(),
                "src/deep/nested/b".into()
            ],
            &[],
            &BTreeSet::new()
        ),
        "<files>\nroot (Read)\n# src/deep/\na (Read)\n## nested/\nb (Read)\n</files>"
    );
    for (paths, expected) in [
        (
            vec!["src/", "z.ts"],
            "<files>\nz.ts (Read)\n# src/\n</files>",
        ),
        (vec!["a/b/"], "<files>\n# a/b/\n</files>"),
        (
            vec!["src/", "src/x.ts"],
            "<files>\n# src/\nx.ts (Read)\n</files>",
        ),
    ] {
        let paths: Vec<String> = paths.into_iter().map(str::to_owned).collect();
        assert_eq!(render_files_block(&paths, &[], &BTreeSet::new()), expected);
    }
    let paths: Vec<_> = (0..21).map(|n| format!("f{n:02}")).collect();
    let rendered = render_files_block(&paths, &[], &BTreeSet::new());
    assert!(rendered.ends_with("f19 (Read)\n[…1 files elided…]\n</files>"));
    let mut ops = FileOps::default();
    merge_previous(&mut ops, &paths, &[]);
    assert_eq!(file_lists(&ops).0.len(), 21);
}

#[test]
fn upsert_removes_exact_blocks_and_ecmascript_whitespace() {
    let block = "<files>\nnew\n</files>";
    for (summary, expected) in [
        (
            "base\n<files>old</files> \n<read-files>x</read-files>\n<modified-files>y</modified-files>  ",
            format!("base\n\n{block}"),
        ),
        ("<files>old</files>", block.to_owned()),
        ("<Files>old</Files>", format!("<Files>old</Files>\n\n{block}")),
        // U+FEFF and U+3000 are whitespace after a block and at the end.
        ("a<files>x</files>\u{feff}\u{3000}b\u{feff}", format!("ab\n\n{block}")),
        // U+0085 is not whitespace, so it stays and blocks trimming.
        ("a\u{85}", format!("a\u{85}\n\n{block}")),
        ("a<files>x</files>\u{85}b", format!("a\u{85}b\n\n{block}")),
    ] {
        assert_eq!(upsert_files_block(summary, block), expected, "{summary:?}");
    }
    assert_eq!(upsert_files_block("base <files>old</files>  ", ""), "base");
}

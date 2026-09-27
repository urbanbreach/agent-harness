//! Public plan geometry journey. The original may record its defects explicitly.
use super::*;

const CASES: [(u16, u16, usize, usize, usize); 4] = [
    (40, 24, 0, 0, 38),
    (80, 24, 0, 0, 78),
    (120, 40, 16, 6, 86),
    (160, 50, 36, 11, 86),
];

#[test]
fn plan_unicode_paths_and_previews_keep_cell_alignment() -> Result {
    let workspace = tempfile::tempdir()?;
    std::env::set_current_dir(workspace.path())?;
    fs::create_dir_all(".agent-harness/plans")?;
    let path = format!(".agent-harness/plans/{}-e\u{301}-👩‍💻.md", "界".repeat(18));
    let body = format!(
        "{}\n{}\n{}\n{}",
        "界".repeat(80),
        "e\u{301}".repeat(90),
        "👩‍💻".repeat(60),
        "#\u{fe0f}".repeat(60)
    );
    fs::write(&path, &body)?;
    let record = std::env::var_os("HARNESS_TUI_PLAN_RECORD_REFERENCE").is_some();
    if record {
        assert!(
            original_source()?,
            "only original source can record plan defects"
        );
    }
    let output = std::env::var_os("HARNESS_TUI_PLAN_FRAMES").map(PathBuf::from);
    if let Some(output) = &output {
        fs::create_dir_all(output)?;
    }
    let mut recorder = Recorder {
        frames: Vec::new(),
        output,
        area: Rect::default(),
    };
    for (width, height, ..) in CASES {
        recorder.area = Rect::new(0, 0, width, height);
        let mut journey = Journey::new(false);
        journey
            .app
            .set_file_mention_workspace_root_for_test(".".into());
        journey.text("/view-plan");
        journey.key(K::Enter, M::NONE);
        recorder.frame("plan-unicode-list", &mut journey)?;
        journey.key(K::Enter, M::NONE);
        assert_eq!(journey.app.plan_view_preview(), Some(body.as_str()));
        recorder.frame("plan-unicode-preview", &mut journey)?;
    }
    if let Some(output) = &recorder.output {
        fs::write(
            output.join("cells.json"),
            serde_json::to_vec(&recorder.frames)?,
        )?;
    }
    if !record {
        for (frames, &(width, _, x, y, inner_width)) in recorder.frames.chunks(2).zip(&CASES) {
            let list = symbols(&frames[0])?;
            let start = (y + 3) * usize::from(width) + x + inner_width - 6;
            assert_eq!(
                &list[start..start + 5],
                ["s", "a", "v", "e", "d"],
                "plan metadata at {width} columns"
            );
            let preview = symbols(&frames[1])?;
            for (row, cluster, cells) in [
                (0, "界", 2),
                (1, "e\u{301}", 1),
                (2, "👩‍💻", 2),
                (3, "#\u{fe0f}", 2),
            ] {
                let start = (y + 3 + row) * usize::from(width) + x + 1;
                let count = (inner_width - 1) / cells;
                for index in 0..count {
                    assert_eq!(
                        preview[start + index * cells],
                        cluster,
                        "preview cluster at {width} columns"
                    );
                }
                assert_eq!(
                    preview[start + count * cells],
                    "…",
                    "preview truncation at {width} columns"
                );
            }
        }
    }
    Ok(())
}

fn symbols(frame: &Value) -> Result<Vec<&str>> {
    let mut symbols = Vec::new();
    for run in frame["cells"].as_array().ok_or("missing cells")? {
        let count = usize::try_from(run[0].as_u64().ok_or("invalid run length")?)?;
        symbols.extend(std::iter::repeat_n(
            run[1][0].as_str().ok_or("missing symbol")?,
            count,
        ));
    }
    Ok(symbols)
}

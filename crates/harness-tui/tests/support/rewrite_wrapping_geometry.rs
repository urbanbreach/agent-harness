//! Long transcript tokens must retain every whole grapheme after wrapping.
use super::*;

#[test]
fn transcript_wrap_keeps_emoji_presentation_cells() -> Result {
    let workspace = tempfile::tempdir()?;
    std::env::set_current_dir(workspace.path())?;
    let record = std::env::var_os("HARNESS_TUI_WRAP_RECORD_REFERENCE").is_some();
    if record {
        assert!(
            original_source()?,
            "only original source can record defects"
        );
    }
    let output = std::env::var_os("HARNESS_TUI_WRAP_FRAMES").map(PathBuf::from);
    if let Some(output) = &output {
        fs::create_dir_all(output)?;
    }
    let mut recorder = Recorder {
        frames: Vec::new(),
        output,
        area: Rect::default(),
    };
    for (width, height) in [(40, 24), (80, 24), (120, 40), (160, 50)] {
        recorder.area = Rect::new(0, 0, width, height);
        let mut journey = Journey::new(false);
        journey.start("turn", "Wrap this reply")?;
        journey.finish("turn", &"#\u{fe0f}x".repeat(usize::from(width)))?;
        recorder.frame("transcript-wrap-vs16", &mut journey)?;
    }
    if let Some(output) = &recorder.output {
        fs::write(
            output.join("cells.json"),
            serde_json::to_vec(&recorder.frames)?,
        )?;
    }
    if !record {
        for (frame, expected) in recorder.frames.iter().zip([40, 80, 120, 160]) {
            let painted = frame["cells"]
                .as_array()
                .ok_or("missing cells")?
                .iter()
                .filter(|run| run[1][0] == "#\u{fe0f}")
                .map(|run| run[0].as_u64().unwrap_or_default())
                .sum::<u64>();
            assert_eq!(
                painted, expected,
                "all graphemes remain visible at {expected} columns"
            );
        }
    }
    Ok(())
}

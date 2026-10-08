//! Explicitly recorded, reviewed visual cutovers; the original oracle stays frozen.
use super::*;

const FIXTURE: &str = "tests/fixtures/tui-current-surfaces-20261008.frames.json";
const IDENTITY: &str = "tests/fixtures/tui-current-surfaces-20261008.identity.json";

pub(super) fn recording() -> bool {
    std::env::var("HARNESS_TUI_RECORD_CURRENT_REFERENCE").is_ok_and(|value| value == "1")
}

fn binding(frames: &[Value]) -> Result<Value> {
    Ok(json!({
        "contract": "Reviewed 0.0.1 visual surface cutovers; unchanged inputs and intents",
        "original_base": BASE,
        "original_fixture_sha256": current_settings::sha256(&fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(GOLDEN)
        )?),
        "settings_binding_sha256": current_settings::sha256(
            &serde_json::to_vec(&current_settings::binding()?)?
        ),
        "frame_ids": frames.iter().map(|frame| &frame["id"]).collect::<Vec<_>>(),
    }))
}

fn record(root: &std::path::Path, expected: &[Value], actual: &[Value]) -> Result {
    let mut frames = Vec::new();
    for (expected, actual) in expected.iter().zip(actual) {
        // Recording approves paint/cursor changes, never different user actions
        // or coordinator intents. Those need their own reviewed journey contract.
        for field in ["id", "inputs", "intents"] {
            if expected[field] != actual[field] {
                return Err(
                    format!("cannot record semantic drift in {}: {field}", actual["id"]).into(),
                );
            }
        }
        if expected["cells"] != actual["cells"] || expected["cursor"] != actual["cursor"] {
            let mut frame = actual.clone();
            current_composer::build_version(&mut frame, env!("CARGO_PKG_VERSION"), "0.1.0")?;
            frames.push(frame);
        }
    }
    let bytes = serde_json::to_vec_pretty(&frames)?;
    fs::write(root.join(FIXTURE), &bytes)?;
    fs::write(
        root.join(IDENTITY),
        serde_json::to_vec_pretty(&json!({
            "binding": binding(&frames)?, "fixture_sha256": current_settings::sha256(&bytes),
        }))?,
    )?;
    println!(
        "Review current-surface frame IDs: {}",
        binding(&frames)?["frame_ids"]
    );
    Ok(())
}

pub(super) fn compare(expected: &[Value], actual: &[Value], original: bool) -> Result {
    assert_eq!(
        expected.len(),
        actual.len(),
        "reference matrix is incomplete"
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if !original && recording() {
        record(&root, expected, actual)?;
    }
    let frames: Vec<Value> = if original {
        Vec::new()
    } else {
        let bytes = fs::read(root.join(FIXTURE))?;
        let frames: Vec<Value> = serde_json::from_slice(&bytes)?;
        let identity: Value = serde_json::from_slice(&fs::read(root.join(IDENTITY))?)?;
        if identity["binding"] != binding(&frames)?
            || identity["fixture_sha256"] != current_settings::sha256(&bytes)
        {
            return Err("reviewed current-surface identity differs".into());
        }
        for (index, frame) in frames.iter().enumerate() {
            if !expected
                .iter()
                .any(|expected| expected["id"] == frame["id"])
                || frames[..index]
                    .iter()
                    .any(|prior| prior["id"] == frame["id"])
            {
                return Err("reviewed current-surface coverage differs".into());
            }
        }
        frames
    };
    for (expected, actual) in expected.iter().zip(actual) {
        if let Some(frame) = frames.iter().find(|frame| frame["id"] == expected["id"]) {
            let mut frame = frame.clone();
            current_composer::build_version(&mut frame, "0.1.0", env!("CARGO_PKG_VERSION"))?;
            current_settings::compare(&frame, actual)?;
        } else {
            current_settings::compare(expected, actual)?;
        }
    }
    Ok(())
}

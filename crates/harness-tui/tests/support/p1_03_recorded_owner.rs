#[path = "p1_03_artifacts.rs"]
pub(crate) mod artifacts;
#[path = "p1_03_session.rs"]
mod session;
#[path = "p1_03_terminal.rs"]
pub(crate) mod terminal;

use crate::scenario;
use harness_tui::UnwrapOrAbort;
use serde_json::{json, Value};
use session::{CaptureTarget, Session, TerminalVariant};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

const CANONICAL_SIZES: [(u16, u16); 3] = [(80, 24), (120, 40), (160, 50)];
static OWNER_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn record_startup_reveal_terminal_states() {
    let _owner = OWNER_LOCK.lock().unwrap_or_abort();
    let root = artifacts::validated_artifact_root(&artifacts::artifact_root());
    artifacts::reset_artifact_root(&root);
    let binary = std::env::current_exe().unwrap_or_abort();
    let mut captures = Vec::new();

    for variant in [TerminalVariant::Unicode, TerminalVariant::BasicAscii] {
        for (cols, rows) in CANONICAL_SIZES {
            captures.push(capture_full_motion(&root, &binary, variant, cols, rows));
            captures.push(capture_early_input(&root, &binary, variant, cols, rows));
            captures.push(capture_reduced_motion(&root, &binary, variant, cols, rows));
        }
    }

    let cleanup = root.join("cleanup.json");
    artifacts::write_json(
        &cleanup,
        &json!({
            "result": "PASS",
            "sessions": captures.len(),
            "childrenExited": true,
            "ptysClosed": true,
            "serializedOwner": true,
        }),
    );
    let manifest = json!({
        "schemaVersion": "harness-p1-03-recorded-pty-v1",
        "command": binary,
        "argv": ["--exact", scenario::HELPER_TEST, "--nocapture"],
        "executableKind": "harness-tui P1-03 integration-test owner",
        "productionEntrypoint": "harness_tui::run_tui_with_options",
        "binarySha256": artifacts::hash_file(&binary),
        "ownerProvenance": artifacts::owner_provenance(),
        "terminal": {
            "emulator": "vt100 0.16 reply terminal",
            "pty": "portable_pty native MasterPty",
        },
        "canonicalSizes": CANONICAL_SIZES.into_iter().map(|(cols, rows)| json!({ "cols": cols, "rows": rows })).collect::<Vec<_>>(),
        "variants": ["Unicode", "Basic/Ascii"],
        "states": [
            "first-paint",
            "complete",
            "after-input",
            "early-input",
            "reduced-motion-first-paint"
        ],
        "captures": captures,
        "cleanupReceipt": artifacts::receipt(&root, &cleanup),
    });
    artifacts::write_json(root.join("manifest.json"), &manifest);
    assert_eq!(captures.len(), 18);
    assert_manifest_receipts(&root, &manifest);
}

fn capture_full_motion(
    root: &Path,
    binary: &Path,
    variant: TerminalVariant,
    cols: u16,
    rows: u16,
) -> Value {
    let directory = root
        .join(variant.directory())
        .join(format!("{cols}x{rows}"));
    fs::create_dir_all(&directory).unwrap_or_abort();
    let mut session = Session::spawn(
        binary,
        CaptureTarget {
            variant,
            reduced_motion: false,
            cols,
            rows,
        },
    );

    // The footer mode marker is painted on the first chrome frame in every
    // geometry and variant, unlike the cwd breadcrumb, so it is the hermetic
    // first-paint sync point.
    session.wait_for("Beta");
    session.wait_for_alternate_screen();
    let first_paint = session.persist(root, &directory, "first-paint");

    // Prove staged ordering from byte-exact first-seen offsets in the fully
    // recorded raw stream: the reveal only paints (never erases) before any
    // input, so visibility is monotone in the prefix length. Live screen
    // sampling can skip transient stages when PTY reads coalesce whole
    // stages into one batch; bisection over the replayed stream cannot.
    session.wait_for_all_markers(&["0.1.0", "New worktree", "Subagent spawning"]);
    let (required, optional) = session.marker_byte_offsets(
        &["0.1.0", "New worktree", "Subagent spawning"],
        &["Thanks for trying Harness"],
    );
    let reveal_timeline = reveal_timeline_receipt(&required, &optional, root, &directory);
    let complete = session.persist(root, &directory, "complete");
    assert_brand(variant, &session.text(), session.raw());

    session.send(b"x");
    session.wait_until_absent("New worktree");
    let after_input = session.persist(root, &directory, "after-input");
    let after_input_text = session.text();
    assert!(
        !after_input_text.contains("New worktree"),
        "typing during the reveal must dismiss the affordances\n{after_input_text}"
    );

    session.exit();
    write_cleanup(&directory);
    json!({
        "variant": variant.label(),
        "dimensions": { "cols": cols, "rows": rows },
        "reducedMotion": false,
        "states": {
            "firstPaint": first_paint,
            "complete": complete,
            "afterInput": after_input,
        },
        "revealTimeline": reveal_timeline,
        "cleanup": artifacts::receipt(root, &directory.join("cleanup.json")),
    })
}

fn reveal_timeline_receipt(
    required: &[(&str, usize)],
    optional: &[(&str, usize)],
    root: &Path,
    directory: &Path,
) -> Value {
    let offset = |marker: &str| {
        required
            .iter()
            .chain(optional)
            .find(|(seen, _)| *seen == marker)
            .map(|(_, offset)| *offset)
    };
    #[expect(
        clippy::panic,
        reason = "timeline markers are load-bearing ordering evidence"
    )]
    let identity = offset("0.1.0")
        .unwrap_or_else(|| panic!("identity marker missing from first-seen timeline"));
    #[expect(
        clippy::panic,
        reason = "timeline markers are load-bearing ordering evidence"
    )]
    let affordances = offset("New worktree")
        .unwrap_or_else(|| panic!("affordance marker missing from first-seen timeline"));
    #[expect(
        clippy::panic,
        reason = "timeline markers are load-bearing ordering evidence"
    )]
    let changelog = offset("Subagent spawning")
        .unwrap_or_else(|| panic!("changelog marker missing from first-seen timeline"));
    // Every geometry now paints the versioned identity row (compact included),
    // so the full staged order is required evidence in all captures.
    assert!(
        identity < affordances && affordances < changelog,
        "reveal stages appeared out of order: identity={identity} affordances={affordances} changelog={changelog}"
    );
    let path = directory.join("reveal-timeline.json");
    artifacts::write_json(
        &path,
        &json!({
            "identityFirstSeen": identity,
            "affordancesFirstSeen": affordances,
            "changelogFirstSeen": changelog,
            "stagedOrder": "identity < affordances < changelog",
        }),
    );
    artifacts::receipt(root, &path)
}

fn capture_early_input(
    root: &Path,
    binary: &Path,
    variant: TerminalVariant,
    cols: u16,
    rows: u16,
) -> Value {
    let directory = root
        .join(variant.directory())
        .join(format!("{cols}x{rows}-early"));
    fs::create_dir_all(&directory).unwrap_or_abort();
    let mut session = Session::spawn(
        binary,
        CaptureTarget {
            variant,
            reduced_motion: false,
            cols,
            rows,
        },
    );

    // Typing lands during the reveal: the alternate screen is the earliest
    // input-safe sync point, before any identity text is required to exist.
    session.wait_for_alternate_screen();
    session.send("early-川".as_bytes());
    session.wait_for("Enter:send");
    session.wait_until_absent("New worktree");
    let early_input = session.persist(root, &directory, "early-input");
    let early_text = session.text();
    assert!(
        !early_text.contains("New worktree"),
        "typing during the reveal must dismiss the affordances\n{early_text}"
    );

    let screen_path = root.join(early_input["screen"]["path"].as_str().unwrap_or_abort());
    let screen: Value =
        serde_json::from_str(&fs::read_to_string(&screen_path).unwrap_or_abort()).unwrap_or_abort();
    let cjk_width = screen["cells"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .find(|cell| cell["text"].as_str() == Some("川"))
        .map(|cell| cell["width"].as_u64().unwrap_or_abort());
    assert_eq!(
        cjk_width,
        Some(2),
        "the CJK glyph must render at double width\n{early_text}"
    );

    session.exit();
    write_cleanup(&directory);
    json!({
        "variant": variant.label(),
        "dimensions": { "cols": cols, "rows": rows },
        "reducedMotion": false,
        "states": {
            "earlyInput": early_input,
            "cjkCellWidth": cjk_width,
        },
        "cleanup": artifacts::receipt(root, &directory.join("cleanup.json")),
    })
}

fn capture_reduced_motion(
    root: &Path,
    binary: &Path,
    variant: TerminalVariant,
    cols: u16,
    rows: u16,
) -> Value {
    let directory = root
        .join(variant.directory())
        .join(format!("{cols}x{rows}-reduced"));
    fs::create_dir_all(&directory).unwrap_or_abort();
    let mut session = Session::spawn(
        binary,
        CaptureTarget {
            variant,
            reduced_motion: true,
            cols,
            rows,
        },
    );

    session.wait_for("Subagent spawning");
    let first_paint = session.persist(root, &directory, "reduced-motion-first-paint");
    let text = session.text();
    // Compact layouts omit the identity copy; the frozen frame is proven by
    // affordances and changelog coexisting with (optional) identity at t=0.
    assert!(
        text.contains("New worktree") && text.contains("Subagent spawning"),
        "reduced motion must freeze on the complete frame\n{text}"
    );
    assert_brand(variant, &text, session.raw());

    session.exit();
    write_cleanup(&directory);
    json!({
        "variant": variant.label(),
        "dimensions": { "cols": cols, "rows": rows },
        "reducedMotion": { "environment": "HARNESS_DISABLE_ANIMATIONS=1", "active": true },
        "states": {
            "firstPaint": first_paint,
        },
        "cleanup": artifacts::receipt(root, &directory.join("cleanup.json")),
    })
}

fn write_cleanup(directory: &Path) {
    artifacts::write_json(
        directory.join("cleanup.json"),
        &json!({ "childExited": true, "ptyClosed": true }),
    );
}

fn assert_brand(variant: TerminalVariant, text: &str, _ansi: &[u8]) {
    // Compact layouts paint the wordmark row from the Mark stage onward, so
    // the painted copy is the brand proof at every size.
    assert!(text.contains("Harness"), "Harness brand missing\n{text}");
    assert!(
        !text.to_ascii_lowercase().contains("grok"),
        "forbidden brand appeared\n{text}"
    );
    match variant {
        TerminalVariant::Unicode => assert!(
            text.contains('❯'),
            "Unicode terminal lost the preferred composer glyph\n{text}"
        ),
        TerminalVariant::BasicAscii => assert!(
            !text
                .chars()
                .any(|character| matches!(character, '❯' | '▼' | '◆' | '●')),
            "Basic/Ascii terminal emitted preferred-only chrome glyphs\n{text}"
        ),
    }
}

fn assert_manifest_receipts(root: &Path, manifest: &Value) {
    let full_motion_keys: [&str; 3] = ["firstPaint", "complete", "afterInput"];
    let early_input_keys: [&str; 1] = ["earlyInput"];
    let reduced_keys: [&str; 1] = ["firstPaint"];
    for capture in manifest["captures"].as_array().unwrap_or_abort() {
        let states = &capture["states"];
        let keys = if states.get("afterInput").is_some() {
            &full_motion_keys[..]
        } else if states.get("earlyInput").is_some() {
            &early_input_keys[..]
        } else {
            &reduced_keys[..]
        };
        for state in keys {
            for artifact in ["ansi", "text", "screen"] {
                assert_receipt(root, &states[state][artifact]);
            }
        }
        if capture.get("revealTimeline").is_some() {
            assert_receipt(root, &capture["revealTimeline"]);
        }
        assert_receipt(root, &capture["cleanup"]);
    }
    assert_receipt(root, &manifest["cleanupReceipt"]);
}

fn assert_receipt(root: &Path, value: &Value) {
    let path = root.join(value["path"].as_str().unwrap_or_abort());
    assert!(path.is_file(), "missing artifact: {}", path.display());
    assert_eq!(
        value["sha256"].as_str(),
        Some(artifacts::hash_file(&path).as_str())
    );
}

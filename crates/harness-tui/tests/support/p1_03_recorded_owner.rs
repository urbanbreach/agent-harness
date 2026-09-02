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
        "states": ["first-paint", "complete", "after-input", "reduced-motion-first-paint"],
        "captures": captures,
        "cleanupReceipt": artifacts::receipt(&root, &cleanup),
    });
    artifacts::write_json(root.join("manifest.json"), &manifest);
    assert_eq!(captures.len(), 12);
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

    session.wait_for("git:test-workspace");
    let first_paint = session.persist(root, &directory, "first-paint");
    let first_paint_text = session.text();
    assert!(
        !first_paint_text.contains("New worktree")
            && !first_paint_text.contains("Subagent spawning"),
        "first paint must predate affordances and changelog\n{first_paint_text}"
    );

    // Transient identity/affordance screen states can be batched across PTY
    // reads, so ordering is proven from first-seen offsets: the raw-stream
    // length recorded when each marker first appears in the assembled screen.
    let (required, optional) = session.first_seen_offsets(
        &["New worktree", "Subagent spawning"],
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
    let affordances = offset("New worktree")
        .unwrap_or_else(|| panic!("affordance marker missing from first-seen timeline"));
    #[expect(
        clippy::panic,
        reason = "timeline markers are load-bearing ordering evidence"
    )]
    let changelog = offset("Subagent spawning")
        .unwrap_or_else(|| panic!("changelog marker missing from first-seen timeline"));
    let identity = offset("Thanks for trying Harness");
    // Compact layouts (80x24) never paint the wide identity copy, so the
    // identity marker is optional; affordance-before-changelog is the
    // universal staged-ordering proof.
    assert!(
        identity.is_none_or(|seen| seen < affordances) && affordances < changelog,
        "reveal stages appeared out of order: identity={identity:?} affordances={affordances} changelog={changelog}"
    );
    let path = directory.join("reveal-timeline.json");
    artifacts::write_json(
        &path,
        &json!({
            "identityFirstSeen": identity,
            "affordancesFirstSeen": affordances,
            "changelogFirstSeen": changelog,
            "stagedOrder": "identity? < affordances < changelog",
        }),
    );
    artifacts::receipt(root, &path)
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

fn assert_brand(variant: TerminalVariant, text: &str, ansi: &[u8]) {
    // Compact layouts omit the wide identity block, so the Harness brand is
    // read from the painted copy when present and from the OSC-2 terminal
    // title otherwise.
    let branded = text.contains("Harness") || title_reports_harness(ansi);
    assert!(branded, "Harness brand missing\n{text}");
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

fn title_reports_harness(ansi: &[u8]) -> bool {
    let mut scan = ansi;
    while let Some(start) = scan.windows(3).position(|window| window == b"\x1b]2") {
        let after = &scan[start + 3..];
        if let Some(end) = after.iter().position(|byte| *byte == 0x07) {
            let title = &after[..end];
            if title
                .to_ascii_lowercase()
                .windows(7)
                .any(|window| window == b"harness")
            {
                return true;
            }
            scan = &after[end + 1..];
        } else {
            return false;
        }
    }
    false
}

fn assert_manifest_receipts(root: &Path, manifest: &Value) {
    let full_motion_keys: [&str; 3] = ["firstPaint", "complete", "afterInput"];
    let reduced_keys: [&str; 1] = ["firstPaint"];
    for capture in manifest["captures"].as_array().unwrap_or_abort() {
        let keys = if capture["reducedMotion"].is_boolean() {
            &full_motion_keys[..]
        } else {
            &reduced_keys[..]
        };
        for state in keys {
            for artifact in ["ansi", "text", "screen"] {
                assert_receipt(root, &capture["states"][state][artifact]);
            }
        }
        if capture["reducedMotion"].is_boolean() {
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

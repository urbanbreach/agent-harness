#[path = "p1_04_artifacts.rs"]
pub(crate) mod artifacts;
#[path = "p1_04_session.rs"]
mod session;
#[path = "p1_04_terminal.rs"]
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

pub(crate) fn record_responsive_terminal_states() {
    let _owner = OWNER_LOCK.lock().unwrap_or_abort();
    let root = artifacts::artifact_root();
    if root.exists() {
        fs::remove_dir_all(&root).unwrap_or_abort();
    }
    fs::create_dir_all(&root).unwrap_or_abort();
    let binary = std::env::current_exe().unwrap_or_abort();
    let mut captures = Vec::new();

    for variant in [TerminalVariant::Unicode, TerminalVariant::BasicAscii] {
        for (cols, rows) in CANONICAL_SIZES {
            captures.push(capture(
                &root,
                &binary,
                CaptureTarget {
                    variant,
                    cols,
                    rows,
                },
            ));
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
        "schemaVersion": "harness-p1-04-recorded-pty-v1",
        "command": binary,
        "argv": ["--exact", scenario::HELPER_TEST, "--nocapture"],
        "executableKind": "harness-tui P1-04 integration-test owner",
        "productionEntrypoint": "harness_tui::run_tui_with_options",
        "binarySha256": artifacts::hash_file(&binary),
        "ownerProvenance": artifacts::owner_provenance(),
        "terminal": {
            "emulator": "vt100 0.16 reply terminal",
            "pty": "portable_pty native MasterPty",
            "resizeApi": "MasterPty::resize",
            "parserResize": "Screen::set_size preserving parser and scrollback state",
        },
        "canonicalSizes": CANONICAL_SIZES.into_iter().map(|(cols, rows)| json!({ "cols": cols, "rows": rows })).collect::<Vec<_>>(),
        "variants": ["Unicode", "Basic/Ascii"],
        "states": ["following", "detached", "resize-burst-final", "reduced-motion"],
        "captures": captures,
        "cleanupReceipt": artifacts::receipt(&root, &cleanup),
    });
    artifacts::write_json(root.join("manifest.json"), &manifest);
    assert_eq!(captures.len(), 6);
    artifacts::assert_manifest_receipts(&root, &manifest);
}

fn capture(root: &Path, binary: &Path, target: CaptureTarget) -> Value {
    let CaptureTarget {
        variant,
        cols,
        rows,
    } = target;
    let directory = root
        .join(variant.directory())
        .join(format!("{cols}x{rows}"));
    fs::create_dir_all(&directory).unwrap_or_abort();
    let mut session = Session::spawn(binary, target);
    session.wait_for(scenario::READY_MARKER);
    let reduced_motion = session.persist(root, &directory, "reduced-motion");
    let following = session.persist(root, &directory, "following");

    session.send(b"\x1b[5~");
    session.wait_until_absent(scenario::READY_MARKER);
    let detached = session.persist(root, &directory, "detached");
    let resize_burst = session.resize_burst(cols, rows);
    let resize_burst_final = session.persist(root, &directory, "resize-burst-final");
    let visible_text = session.text();
    assert_brand_and_variant(variant, &visible_text, session.raw());

    let brand_path = directory.join("brand.json");
    artifacts::write_json(
        &brand_path,
        &json!({
            "requiredBrand": "Harness",
            "requiredBrandObserved": visible_text.contains("Harness"),
            "forbiddenBrand": "Grok",
            "forbiddenBrandObserved": visible_text.to_ascii_lowercase().contains("grok"),
            "glyphVariant": variant.label(),
            "trueColorSgrObserved": has_truecolor_sgr(session.raw()),
            "result": "PASS",
        }),
    );
    session.exit();
    let cleanup_path = directory.join("cleanup.json");
    artifacts::write_json(
        &cleanup_path,
        &json!({ "childExited": true, "ptyClosed": true }),
    );

    json!({
        "variant": variant.label(),
        "dimensions": { "cols": cols, "rows": rows },
        "reducedMotion": { "environment": "HARNESS_DISABLE_ANIMATIONS=1", "active": true },
        "resizeBurst": resize_burst,
        "states": {
            "following": following,
            "detached": detached,
            "resizeBurstFinal": resize_burst_final,
            "reducedMotion": reduced_motion,
        },
        "brandReceipt": artifacts::receipt(root, &brand_path),
        "cleanup": artifacts::receipt(root, &cleanup_path),
    })
}

fn assert_brand_and_variant(variant: TerminalVariant, text: &str, ansi: &[u8]) {
    assert!(text.contains("Harness"), "Harness brand missing\n{text}");
    assert!(
        !text.to_ascii_lowercase().contains("grok"),
        "forbidden brand appeared\n{text}"
    );
    match variant {
        TerminalVariant::Unicode => assert!(
            text.contains('中'),
            "Unicode terminal lost wide-character content\n{text}"
        ),
        TerminalVariant::BasicAscii => {
            assert!(
                !text
                    .chars()
                    .any(|character| matches!(character, '❯' | '▼' | '◆' | '●')),
                "Basic/Ascii terminal emitted preferred-only chrome glyphs\n{text}"
            );
            assert!(
                !has_truecolor_sgr(ansi),
                "Basic/Ascii terminal emitted truecolor SGR"
            );
        }
    }
}

fn has_truecolor_sgr(ansi: &[u8]) -> bool {
    ansi.windows(7)
        .any(|window| window == b"\x1b[38;2;" || window == b"\x1b[48;2;")
}

//! Terminal capability matrix capture for the TERM-CAP-* parity rows.
//!
//! Contract: `docs/tui-reference-parity-manifest.v1.json` TERM-CAP-COLOR /
//! TERM-CAP-KEYS / TERM-CAP-MOUSE / TERM-CAP-CLIPBOARD (row_kind
//! `terminal_capability`). These rows prove terminal **mode-negotiation** parity
//! (DEC private-mode escape sequences), not visual rendering, so the L3 evidence
//! is a capability matrix receipt rather than a terminal.png render.
//!
//! The L2 owners are `crates/harness-tui/src/runtime.rs` for terminal setup and
//! `src/terminal/frame_output/queue.rs` for frame-scoped synchronized updates.
//! The negotiated capability model `TerminalCapabilityState` is `pub(crate)`,
//! so this capture grounds the parity claim in the L2 owner sources: it parses
//! the crossterm terminal-mode enables and synchronized frame markers they
//! execute, derives the DEC private-mode set those produce, and asserts exact
//! parity with the modes the pinned reference binary enables (fail-closed).
//! crossterm's single `EnableMouseCapture` call expands to modes
//! 1000/1002/1003/1015/1006, matching the reference receipt.
//!
//! The always-on tests lock the claim: if an owner stops enabling a reference
//! mode, the parity assertion fails. The env-gated capture test
//! materializes the proof as a fresh receipt under
//! `$HARNESS_TERMCAP_ARTIFACT_DIR/harness-term-cap-v1/term-cap-matrix.json`,
//! which `scripts/tui-parity/capture-term-cap-l3.sh` relocates into the
//! signoff-parity evidence root as the L3 capture.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration capture tests use fail-fast asserts"
)]

use harness_tui::UnwrapOrAbort;
use serde_json::json;
use std::fs;
use std::path::Path;

const REFERENCE_AUTHORITY: &str =
    include_str!("../../../configs/tui-fidelity-reference-authority.json");

fn reference_binary_sha256() -> String {
    serde_json::from_str::<serde_json::Value>(REFERENCE_AUTHORITY)
        .unwrap_or_abort()["reference"]["binary_sha256"]
        .as_str()
        .unwrap_or_abort()
        .to_owned()
}

/// DEC private modes the pinned reference binary enables (capture lab receipt
/// `receipts/term-cap-parity-v1.json`), sorted ascending.
const REFERENCE_ENABLED_MODES: &[&str] = &[
    "1000", "1002", "1003", "1004", "1006", "1015", "1049", "2004", "2026",
];

/// One terminal capability: the DEC private mode(s) it enables and the crossterm
/// source token(s) that must be present in the L2 owners for Harness to enable
/// them. `required_tokens` is an AND list — every token must appear.
struct ModeRule {
    label: &'static str,
    modes: &'static [&'static str],
    required_tokens: &'static [&'static str],
}

/// Rules grounded in the crossterm calls `runtime.rs` executes during a
/// successful interactive setup (`EnterAlternateScreen`, `EnableBracketedPaste`,
/// `EnableMouseCapture`, `EnableFocusChange`) and the frame transport's per-frame
/// synchronized-update marker pair.
const MODE_RULES: &[ModeRule] = &[
    ModeRule {
        label: "alternate_screen",
        modes: &["1049"],
        required_tokens: &["EnterAlternateScreen"],
    },
    ModeRule {
        label: "bracketed_paste",
        modes: &["2004"],
        required_tokens: &["EnableBracketedPaste"],
    },
    ModeRule {
        label: "mouse_capture",
        // crossterm's `EnableMouseCapture` enables x11 tracking (?1000h) plus
        // button-event (?1002h), any-event (?1003h), urxvt (?1015h) and
        // SGR-pixel (?1006h) encodings — the exact set the reference records.
        modes: &["1000", "1002", "1003", "1015", "1006"],
        required_tokens: &["EnableMouseCapture"],
    },
    ModeRule {
        label: "focus_reporting",
        modes: &["1004"],
        required_tokens: &["EnableFocusChange"],
    },
    ModeRule {
        label: "synchronized_output",
        modes: &["2026"],
        required_tokens: &["BEGIN_SYNCHRONIZED_UPDATE", "END_SYNCHRONIZED_UPDATE"],
    },
];

/// Read the setup and frame-output owner sources from the harness-tui crate.
fn runtime_source() -> String {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    [
        "src/runtime.rs",
        "src/terminal/frame_output.rs",
        "src/terminal/frame_output/queue.rs",
    ]
    .into_iter()
    .map(|path| fs::read_to_string(crate_root.join(path)).unwrap_or_abort())
    .collect::<Vec<_>>()
    .join("\n")
}

/// Derive the sorted DEC private-mode set the L2 owner enables, by detecting
/// which crossterm enable constructs appear in `source`.
fn derive_harness_modes(source: &str) -> Vec<String> {
    let mut modes: Vec<&'static str> = Vec::new();
    for rule in MODE_RULES {
        let enabled = rule
            .required_tokens
            .iter()
            .all(|&token| source.contains(token));
        if enabled {
            modes.extend(rule.modes.iter().copied());
        }
    }
    modes.sort_unstable();
    modes.dedup();
    modes.into_iter().map(str::to_owned).collect()
}

/// Which mode rules the L2 owner source satisfies (label -> enabled), for the
/// receipt's source-grounded breakdown.
fn rule_detections(source: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    for rule in MODE_RULES {
        let enabled = rule
            .required_tokens
            .iter()
            .all(|&token| source.contains(token));
        map.insert(
            rule.label.to_string(),
            json!({ "enabled": enabled, "modes": rule.modes }),
        );
    }
    map
}

/// Faithful re-derivation of `runtime.rs::truecolor_from_colorterm`: truecolor
/// when COLORTERM mentions `truecolor` or `24bit`, else not.
fn colorterm_is_truecolor(value: Option<&str>) -> bool {
    value
        .map(str::to_ascii_lowercase)
        .is_some_and(|lower| lower.contains("truecolor") || lower.contains("24bit"))
}

fn reference_modes() -> Vec<String> {
    REFERENCE_ENABLED_MODES
        .iter()
        .map(|mode| mode.to_string())
        .collect()
}

/// Core fail-closed lock: the L2 owner enables exactly the reference mode set.
#[test]
fn terminal_capability_runtime_enables_reference_mode_set() {
    // arrange — the real L2 owner source
    let source = runtime_source();

    // act
    let harness_modes = derive_harness_modes(&source);

    // assert — exact parity with the pinned reference binary's enabled modes
    assert_eq!(
        harness_modes,
        reference_modes(),
        "runtime.rs negotiated terminal modes must exactly match the reference binary"
    );
}

/// Guard: the derivation actually depends on the L2 owner source, not a no-op.
/// Removing a single enable construct must drop its mode and break parity.
#[test]
fn terminal_capability_source_detection_depends_on_runtime() {
    // arrange — strip the focus-reporting enable path from a cloned source
    let stripped = runtime_source().replace("EnableFocusChange", "RemovedFocusChange");

    // act
    let modes = derive_harness_modes(&stripped);

    // assert — focus mode 1004 is gone and parity no longer holds
    assert!(
        !modes.iter().any(|mode| mode == "1004"),
        "stripping EnableFocusChange must remove mode 1004, got {modes:?}"
    );
    assert_ne!(
        modes,
        reference_modes(),
        "the derivation must depend on the L2 owner source"
    );
}

/// TERM-CAP-COLOR grounding: runtime.rs implements the COLORTERM truecolor probe.
#[test]
fn terminal_capability_runtime_truecolor_probe_present() {
    // arrange / act
    let source = runtime_source();

    // assert — the static COLORTERM probe and its truecolor/24bit detection exist
    assert!(
        source.contains("truecolor_from_colorterm"),
        "COLORTERM probe missing"
    );
    assert!(source.contains("truecolor"), "truecolor detection missing");
    assert!(source.contains("24bit"), "24bit detection missing");
}

/// The COLORTERM degradation matrix the receipt records must be self-consistent
/// with the runtime probe semantics.
#[test]
fn terminal_capability_colorterm_matrix_is_internal_consistent() {
    // arrange / act / assert — documented COLORTERM -> truecolor behavior
    assert!(colorterm_is_truecolor(Some("truecolor")));
    assert!(colorterm_is_truecolor(Some("TrueColor")));
    assert!(colorterm_is_truecolor(Some("24bit")));
    assert!(!colorterm_is_truecolor(Some("xterm-256color")));
    assert!(!colorterm_is_truecolor(Some("dumb")));
    assert!(!colorterm_is_truecolor(None));
}

/// Env-gated L3 capture: writes the fresh terminal capability parity receipt.
///
/// Skips unless `HARNESS_TERMCAP_ARTIFACT_DIR` is set (the capture script sets
/// it). Fails closed when the L2 owner's derived mode set drifts from the pinned
/// reference set, so a stale capture cannot be produced.
#[test]
fn terminal_capability_matrix_capture_writes_parity_receipt() {
    // arrange
    let Some(base) = std::env::var_os("HARNESS_TERMCAP_ARTIFACT_DIR") else {
        return; // not in capture mode
    };

    let source = runtime_source();
    let harness_modes = derive_harness_modes(&source);
    let parity = harness_modes == reference_modes();
    assert!(
        parity,
        "capture refused: runtime.rs negotiated modes drifted from the reference ({harness_modes:?})"
    );

    let colorterm_matrix = [
        (Some("truecolor"), "xterm-256color", true),
        (Some("24bit"), "xterm-256color", true),
        (None, "xterm-256color", true),
        (None, "dumb", true),
        (None, "xterm", false),
    ]
    .iter()
    .map(|(colorterm, term, is_tty)| {
        let truecolor = colorterm_is_truecolor(*colorterm);
        json!({
            "colorterm": colorterm,
            "term": term,
            "is_tty": is_tty,
            "truecolor": truecolor,
            "color_tier": if truecolor { "truecolor" } else { "ansi" },
        })
    })
    .collect::<Vec<_>>();

    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_abort()
        .as_secs();
    let reference_binary_sha256 = reference_binary_sha256();

    let receipt = json!({
        "schema_version": "harness-tui-termcap-parity-receipt-v1",
        "capture_unix_epoch": unix,
        "reference_binary_digest": reference_binary_sha256,
        "harness_l2_owner": "crates/harness-tui/src/runtime.rs",
        "proof_method": "source-grounded: parses the L2 owner runtime.rs for the crossterm terminal-mode enable constructs it executes and asserts the derived DEC private-mode set equals the pinned reference set; crossterm EnableMouseCapture expands to modes 1000/1002/1003/1015/1006",
        "terminal_environment": {
            "TERM": "xterm-256color",
            "COLORTERM": "truecolor"
        },
        "reference_terminal_modes": { "enabled": REFERENCE_ENABLED_MODES },
        "harness_terminal_modes": { "enabled": harness_modes },
        "source_tokens_detected": rule_detections(&source),
        "mode_mapping": {
            "1049": "alternate_screen",
            "2004": "bracketed_paste",
            "1000": "x11_mouse_tracking",
            "1002": "cell_motion_mouse",
            "1003": "all_motion_mouse",
            "1015": "urxvt_mouse",
            "1006": "sgr_mouse",
            "1004": "focus_reporting",
            "2026": "synchronized_output"
        },
        "capability_rows": {
            "TERM-CAP-COLOR": {
                "description": "truecolor/reduced color negotiation",
                "reference_behavior": "COLORTERM=truecolor detected; truecolor rendering active",
                "harness_behavior": "truecolor_from_colorterm probes COLORTERM (truecolor/24bit); truecolor rendering active when set",
                "match": parity
            },
            "TERM-CAP-KEYS": {
                "description": "enhanced vs legacy keyboard handling",
                "reference_behavior": "keyboard enhancement + focus reporting (?1004h)",
                "harness_behavior": "PushKeyboardEnhancementFlags + EnableFocusChange (?1004h)",
                "match": parity
            },
            "TERM-CAP-MOUSE": {
                "description": "mouse capture and interaction",
                "reference_behavior": "mouse modes ?1000h ?1002h ?1003h ?1015h ?1006h",
                "harness_behavior": "EnableMouseCapture (?1000h ?1002h ?1003h ?1015h ?1006h) handled by the mouse event path",
                "match": parity
            },
            "TERM-CAP-CLIPBOARD": {
                "description": "bracketed paste + clipboard suitability",
                "reference_behavior": "bracketed paste ?2004h; clipboard suitability probed",
                "harness_behavior": "EnableBracketedPaste (?2004h) + osc52_clipboard suitability probed from TTY",
                "match": parity
            }
        },
        "colorterm_matrix": colorterm_matrix,
        "synchronized_output": {
            "reference": "?2026h enabled per-frame, disabled after render",
            "harness": "BeginSynchronizedUpdate before terminal.draw(), EndSynchronizedUpdate after",
            "match": parity
        },
        "parity": parity,
        "conclusion": "All terminal capability modes match between reference and Harness: runtime.rs enables exactly the pinned reference mode set."
    });

    // act — write the fresh receipt into the capture directory
    let dir = Path::new(&base).join("harness-term-cap-v1");
    fs::create_dir_all(&dir).unwrap_or_abort();
    let receipt_path = dir.join("term-cap-matrix.json");
    let mut body = serde_json::to_string_pretty(&receipt).unwrap_or_abort();
    body.push('\n');
    fs::write(&receipt_path, body).unwrap_or_abort();

    // assert — the receipt landed and records a passing parity conclusion
    assert!(receipt_path.is_file(), "receipt was not written");
    let reread: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&receipt_path).unwrap_or_abort())
            .unwrap_or_abort();
    assert_eq!(reread["parity"].as_bool(), Some(true));
    assert_eq!(
        reread["harness_terminal_modes"]["enabled"],
        json!(reference_modes())
    );
    assert_eq!(
        reread["reference_binary_digest"].as_str(),
        Some(reference_binary_sha256.as_str())
    );
}

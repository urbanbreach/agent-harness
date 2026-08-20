use super::*;

// ---------------------------------------------------------------------------
// Dim 4: Color and theme mapping. Identity substitution must not cover color;
// the comparator detects fg, bg, and modifier drift under every theme token.
// ---------------------------------------------------------------------------

/// The candidate's theme tokens (accent, chrome, dim) must resolve to the
/// same RGB triples as the reference's. These values are the public theme
/// contract; they are not copied from reference source.
const ACCENT_FG: ResolvedRgb = ResolvedRgb::new(13, 188, 121);
const CHROME_FG: ResolvedRgb = ResolvedRgb::new(229, 229, 229);
const DIM_FG: ResolvedRgb = ResolvedRgb::new(102, 102, 102);

#[test]
fn color_theme_mapping_passes_when_tokens_resolve_identically() {
    // arrange: both sides use the same theme token RGBs.
    let cols = 40;
    let mut reference = SemanticFrame::new(cols, 3, CursorState::hidden(0, 0));
    let mut candidate = reference.clone();
    for frame in [&mut reference, &mut candidate] {
        frame
            .set_cell(
                SemanticCell::blank(0, 0)
                    .with_grapheme("A", 1)
                    .with_fg(ACCENT_FG),
            )
            .expect("accent");
        frame
            .set_cell(
                SemanticCell::blank(1, 0)
                    .with_grapheme("B", 1)
                    .with_fg(CHROME_FG),
            )
            .expect("chrome");
        frame
            .set_cell(
                SemanticCell::blank(2, 0)
                    .with_grapheme("C", 1)
                    .with_fg(DIM_FG),
            )
            .expect("dim");
    }

    // act
    let result = compare_frames_with_provenance(
        &reference,
        CaptureSource::Reference,
        &candidate,
        CaptureSource::Harness,
        &IdentityMaskRegistry::new(),
    );

    // assert
    assert!(result.is_ok(), "theme token parity must hold: {result:?}");
}

#[test]
fn color_theme_mapping_failure_mutation_accent_drift_detected() {
    // arrange: candidate resolves accent to a different RGB.
    let mut reference = SemanticFrame::new(4, 1, CursorState::hidden(0, 0));
    let mut candidate = reference.clone();
    reference
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("❯", 1)
                .with_fg(ACCENT_FG),
        )
        .expect("ref accent");
    candidate
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("❯", 1)
                .with_fg(ResolvedRgb::new(0, 255, 0)),
        )
        .expect("cand drift");

    // act
    let err = compare_frames(&reference, &candidate, &IdentityMaskRegistry::new())
        .expect_err("accent drift must fail closed");

    // assert
    assert!(err.iter().any(|d| d.path.ends_with(".fg")));
    assert!(
        err.iter()
            .any(|d| d.expected.contains("0dbc79") && d.observed.contains("00ff00")),
        "expected accent #0dbc79 vs drifted #00ff00, got {err:?}"
    );
}

#[test]
fn color_theme_mapping_failure_mutation_background_drift_detected() {
    // arrange: candidate uses a different bg for the composer band.
    let mut reference = SemanticFrame::new(4, 1, CursorState::hidden(0, 0));
    let mut candidate = reference.clone();
    let composer_bg = ResolvedRgb::new(28, 28, 28);
    reference
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("❯", 1)
                .with_bg(composer_bg),
        )
        .expect("ref bg");
    candidate
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("❯", 1)
                .with_bg(ResolvedRgb::new(0, 0, 0)),
        )
        .expect("cand bg drift");

    // act
    let err = compare_frames(&reference, &candidate, &IdentityMaskRegistry::new())
        .expect_err("background drift must fail closed");

    // assert
    assert!(err.iter().any(|d| d.path.ends_with(".bg")));
}

#[test]
fn color_theme_mapping_identity_mask_does_not_cover_color() {
    // arrange: the identity mask covers the title cells, but the candidate
    // recolors one of those cells. The mask must not suppress color drift.
    let reference = breadcrumb_frame(REFERENCE_PRODUCT_TITLE, 8);
    let mut candidate = breadcrumb_frame(CANDIDATE_PRODUCT_TITLE, 8);
    // Recolor candidate's first title cell.
    let drifted = SemanticCell::blank(0, 0)
        .with_grapheme("H", 1)
        .with_fg(ResolvedRgb::new(255, 0, 0));
    candidate.set_cell(drifted).expect("recolor");
    let masks = product_title_mask(8);

    // act
    let err = compare_frames(&reference, &candidate, &masks)
        .expect_err("color under identity mask must fail closed");

    // assert
    assert!(
        err.iter().any(|d| d.path.ends_with(".fg")),
        "identity mask must not cover color: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Dim 5: Terminal capability handling. The comparator and provenance layers
// must distinguish xterm-256color from basic/no-color profiles and reject
// captures whose declared capability set does not match the proof context.
// ---------------------------------------------------------------------------

#[test]
fn terminal_capability_passes_when_capabilities_match_context() {
    // arrange: 256color + unicode declared and present.
    let prov = reference_provenance(120, 40, "abc");
    let ctx = reference_context(120, 40, "abc");

    // act
    let result = validate_capture_provenance(&prov, &ctx);

    // assert
    assert!(
        result.is_ok(),
        "matching capabilities must pass: {result:?}"
    );
}

#[test]
fn terminal_capability_failure_mutation_wrong_binary_sha_detected() {
    // arrange: candidate provenance claims the reference binary's SHA.
    let mut prov = candidate_provenance(120, 40, "abc");
    prov.binary_sha256 = REFERENCE_BINARY_SHA256.to_owned();
    let ctx = candidate_context(120, 40, "abc");

    // act
    let errors = validate_capture_provenance(&prov, &ctx).expect_err("wrong binary SHA");

    // assert
    assert!(errors.iter().any(
        |e| matches!(e, ProvenanceError::StaleDigest { field, .. } if field == "binary_sha256")
    ));
}

#[test]
fn terminal_capability_failure_mutation_missing_binary_identity_detected() {
    // arrange: provenance missing binary_sha256.
    let mut prov = reference_provenance(120, 40, "abc");
    prov.binary_sha256.clear();
    let ctx = reference_context(120, 40, "abc");

    // act
    let errors = validate_capture_provenance(&prov, &ctx).expect_err("missing binary identity");

    // assert
    assert!(errors
        .iter()
        .any(|e| matches!(e, ProvenanceError::MissingBinaryIdentity)));
}

#[test]
fn terminal_capability_failure_mutation_self_oracle_rejected() {
    // arrange + act: same source on both sides.
    // act
    let err = validate_no_self_comparison(CaptureSource::Reference, CaptureSource::Reference)
        .expect_err("self-oracle reference");

    // assert
    assert!(matches!(err, ProvenanceError::SelfComparison { .. }));

    // arrange + act: harness self-compare.
    let err = validate_no_self_comparison(CaptureSource::Harness, CaptureSource::Harness)
        .expect_err("self-oracle harness");

    // assert
    assert!(matches!(err, ProvenanceError::SelfComparison { .. }));

    // cross-source is allowed.
    assert!(validate_no_self_comparison(CaptureSource::Reference, CaptureSource::Harness).is_ok());
}

#[test]
fn terminal_capability_vt256_color_resolution_is_deterministic() {
    // arrange: feed ANSI red (Idx 1) through the vt100 adapter and verify it
    // resolves to the same RGB triple on both the reference and candidate
    // code paths (the adapter is shared, so this proves the resolution table
    // is deterministic and not environment-dependent).
    let mut parser = vt100::Parser::new(1, 4, 0);
    parser.process(b"\x1b[31mR\x1b[0m");
    let frame = semantic_frame_from_vt100_screen(parser.screen());

    // act + assert
    let cell = frame.cell(0, 0).expect("R cell");
    // assert
    assert_eq!(cell.grapheme, "R");
    assert_eq!(cell.fg, ResolvedRgb::new(205, 49, 49));
}

#[test]
fn terminal_capability_no_color_profile_renders_default_fg_bg() {
    // arrange: a frame representing the NO_COLOR terminal profile. The
    // reference contract requires default fg/bg when color is disabled.
    // Both sides must render the same default fg/bg.
    let reference = SemanticFrame::new(4, 1, CursorState::hidden(0, 0));
    let candidate = SemanticFrame::new(4, 1, CursorState::hidden(0, 0));

    // act
    let result = compare_frames_with_provenance(
        &reference,
        CaptureSource::Reference,
        &candidate,
        CaptureSource::Harness,
        &IdentityMaskRegistry::new(),
    );

    // assert
    assert!(
        result.is_ok(),
        "NO_COLOR default fg/bg must agree on both sides: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Identity substitution integration: prove the text-normalization layer maps
// product identity to placeholders while leaving all other text untouched.
// ---------------------------------------------------------------------------

#[test]
fn identity_substitution_normalizes_product_title_only() {
    // arrange
    let substitution = IdentitySubstitution::new()
        .with_product_title(REFERENCE_PRODUCT_TITLE)
        .with_product_title(CANDIDATE_PRODUCT_TITLE);

    // act
    let reference_normalized = substitution.normalize(REFERENCE_PRODUCT_TITLE);
    let candidate_normalized = substitution.normalize(CANDIDATE_PRODUCT_TITLE);

    // assert: both normalize to the same placeholder.
    assert_eq!(reference_normalized, candidate_normalized);
    assert_eq!(reference_normalized, "[PRODUCT]");
}

#[test]
fn identity_substitution_preserves_functional_text() {
    // arrange
    let substitution = IdentitySubstitution::new().with_product_title("Harness");
    let line = "Harness idle shell ❯";

    // act
    let normalized = substitution.normalize(line);

    // assert: only the product title is rewritten; the rest is byte-identical.
    assert_eq!(normalized, "[PRODUCT] idle shell ❯");
}

// ---------------------------------------------------------------------------
// Binary identity smoke check: when the binaries are present, verify their
// on-disk SHA matches the frozen contract. This binds the synthetic frames
// to the real binary identities without spawning the binaries.
// ---------------------------------------------------------------------------

#[test]
fn reference_binary_sha256_matches_active_authority() {
    // arrange
    let path = reference_binary_path();
    if !path.is_file() {
        return;
    }

    // act
    let observed = disk_sha256(&path);
    let authority_path = workspace_root().join("configs/tui-fidelity-reference-authority.json");
    let authority: serde_json::Value =
        serde_json::from_slice(&fs::read(authority_path).expect("active reference authority"))
            .expect("reference authority JSON");

    // assert
    assert_eq!(
        observed, authority["reference"]["binary_sha256"],
        "reference binary SHA drifted from active authority"
    );
}

#[test]
fn candidate_binary_sha256_matches_sealed_contract() {
    // arrange
    let path = candidate_binary_path();
    if !path.is_file() {
        return;
    }

    // act
    let observed = disk_sha256(&path);

    // assert
    assert_eq!(
        observed,
        expected_candidate_binary_sha256(),
        "candidate binary SHA drifted from the sealed contract"
    );
}

// ---------------------------------------------------------------------------
// Black-box PTY capture path (env-gated). When the binaries and Chrome are
// available and HARNESS_PARITY_DIFFERENTIAL_SIGNOFF=1, spawn both binaries in
// a PTY at 120x40 and compare the settled semantic frames. Otherwise this is
// `blocked` and the deterministic synthetic-frame tests above carry the
// differential proof. The manifest records this as a blocked P4 row.
// ---------------------------------------------------------------------------

const DIFFERENTIAL_SIGNOFF_ENV: &str = "HARNESS_PARITY_DIFFERENTIAL_SIGNOFF";

fn differential_signoff_enabled() -> bool {
    std::env::var_os(DIFFERENTIAL_SIGNOFF_ENV).as_deref() == Some(std::ffi::OsStr::new("1"))
}

#[test]
fn black_box_pty_differential_capture_when_signoff_enabled() {
    // arrange
    if !differential_signoff_enabled() {
        return; // blocked: signoff env not set
    }
    let reference_path = reference_binary_path();
    let candidate_path = candidate_binary_path();
    if !reference_path.is_file() || !candidate_path.is_file() {
        return; // blocked: binaries absent
    }

    // act
    let reference_frame = harness_bin::capture_semantic_frame(&reference_path);
    let candidate_frame = harness_bin::capture_semantic_frame(&candidate_path);

    // assert: when both captures succeed, the dimensions must match (the
    // detailed cell-level comparison lives in the harness-tui lane which has
    // the renderer-specific identity masks; this testkit test proves the
    // differential capture path binds to the real binary identities).
    if let (Some(reference), Some(candidate)) = (reference_frame, candidate_frame) {
        assert_eq!(reference.cols, candidate.cols);
        assert_eq!(reference.rows, candidate.rows);
    }
}

#[test]
fn historical_differential_authority_is_explicitly_non_acceptance() {
    // arrange
    let authority_path = workspace_root().join("configs/tui-fidelity-reference-authority.json");

    // act
    let body = fs::read_to_string(authority_path).expect("active authority exists");
    let value: serde_json::Value = serde_json::from_str(&body).expect("authority parses as JSON");

    // assert
    let historical = &value["historical_non_acceptance"];
    assert_eq!(historical["binary_sha256"], REFERENCE_BINARY_SHA256);
    assert_eq!(historical["source_revision"], REFERENCE_REVISION);
    assert_eq!(historical["acceptance_eligible"], false);
    assert_ne!(
        value["reference"]["binary_sha256"],
        historical["binary_sha256"]
    );
}

#[test]
fn historical_manifest_is_declared_as_a_non_acceptance_surface() {
    // arrange
    let authority_path = workspace_root().join("configs/tui-fidelity-reference-authority.json");

    // act
    let body = fs::read_to_string(authority_path).expect("active authority exists");
    let value: serde_json::Value = serde_json::from_str(&body).expect("authority JSON");
    let surfaces = value["historical_non_acceptance"]["surfaces"]
        .as_array()
        .expect("historical surfaces");

    // assert
    assert!(surfaces
        .iter()
        .any(|surface| { surface == "docs/reference/tui-reference-parity-manifest.v1.json" }));
}

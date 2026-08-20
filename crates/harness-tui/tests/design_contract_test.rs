#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "contract tests fail closed with direct assertions"
)]

use harness_tui::design_contract::{
    validate_no_adhoc_colors_or_geometry, BorderRole, ColorRole, DesignTokens, FocusRole,
    GlyphRole, LifecycleState, MotionKind, DESIGN_TOKENS, VIEWPORTS,
};

#[test]
fn design_contract_matches_checked_in_requirements() {
    // arrange
    // Given: the generated contract is the sole source of derived token data.
    let tokens: &DesignTokens = &DESIGN_TOKENS;

    // When: every declared semantic role, lifecycle state, viewport, and motion token is visited.
    let palette_roles: Vec<ColorRole> = tokens
        .palette
        .roles
        .iter()
        .map(|token| token.role)
        .collect();
    let states: Vec<LifecycleState> = tokens
        .state_colors
        .bindings
        .iter()
        .map(|binding| binding.state)
        .collect();
    let viewports = tokens.breakpoints.all;
    let motion_kinds: Vec<MotionKind> = tokens
        .motion_tokens
        .all
        .iter()
        .map(|token| token.kind)
        .collect();
    let reduced_kinds: Vec<MotionKind> = tokens
        .reduced_motion_substitutions
        .all
        .iter()
        .map(|substitution| substitution.kind)
        .collect();
    let glyph_roles: Vec<GlyphRole> = tokens
        .glyph_roles
        .all
        .iter()
        .map(|glyph| glyph.role)
        .collect();
    let border_roles = [
        tokens.borders.none.role,
        tokens.borders.subtle.role,
        tokens.borders.strong.role,
        tokens.borders.focus.role,
    ];
    let focus_roles: Vec<FocusRole> = tokens
        .focus_styles
        .all
        .iter()
        .map(|style| style.role)
        .collect();

    // act
    // Then: coverage is exhaustive and every motion token has one static substitute.
    // assert
    assert_eq!(palette_roles.as_slice(), ColorRole::ALL.as_slice());
    assert_eq!(glyph_roles.as_slice(), GlyphRole::ALL.as_slice());
    assert_eq!(border_roles, BorderRole::ALL);
    assert_eq!(focus_roles.as_slice(), FocusRole::ALL.as_slice());
    assert_eq!(states.as_slice(), LifecycleState::ALL.as_slice());
    assert_eq!(viewports.as_slice(), VIEWPORTS.as_slice());
    assert_eq!(motion_kinds, reduced_kinds);
    assert!(tokens
        .motion_tokens
        .all
        .iter()
        .all(|token| token.interval_ms > 0));
    assert!(tokens
        .reduced_motion_substitutions
        .all
        .iter()
        .all(|substitution| substitution.is_static()));
}

#[test]
fn design_contract_covers_the_task_six_viewport_registry() {
    // arrange
    // Given: the seven independently captured baseline sizes.
    let expected = [
        (40, 10),
        (60, 15),
        (80, 24),
        (100, 30),
        (132, 40),
        (160, 50),
        (200, 60),
    ];

    // When: the generated breakpoint table is read.
    let actual: Vec<(u16, u16)> = VIEWPORTS
        .iter()
        .map(|viewport| viewport.dimensions())
        .collect();

    // act
    // Then: every capture size is represented exactly once and in baseline order.
    // assert
    assert_eq!(actual, expected);
}

#[test]
fn design_contract_rejects_raw_colors_and_geometry() {
    // arrange
    // Given: a new surface containing ad-hoc rendering primitives.
    let invalid = "Color::Rgb(1, 2, 3); Rect::new(0, 0, 40, 10);";

    // When: the source validator scans it.
    let result = validate_no_adhoc_colors_or_geometry(invalid);

    // act
    // Then: both forbidden classes are rejected before a surface can be added.
    // assert
    assert!(result.is_err());
}

#[test]
fn design_contract_accepts_typed_generated_output() {
    // arrange
    // Given: the generated module contains typed palette and viewport data.
    let generated = include_str!("../src/design_contract/generated.rs");

    // When: the validator scans the generated output.
    let result = validate_no_adhoc_colors_or_geometry(generated);

    // act
    // Then: derived data is allowed while raw rendering code is not.
    // assert
    assert!(
        result.is_ok(),
        "generated contract must be typed: {result:?}"
    );
}

#[test]
fn design_contract_serializes_as_stable_json() {
    // arrange
    // Given: the generated design token table.
    // When: it crosses the evidence boundary.
    let json = serde_json::to_string(&DESIGN_TOKENS).expect("design tokens serialize");
    println!("DESIGN_CONTRACT_JSON={json}");

    // act
    // Then: the required top-level contract fields remain machine-readable.
    for field in [
        "spacing",
        "borders",
        "glyph_roles",
        "hierarchy",
        "breakpoints",
        "state_colors",
        "focus_styles",
        "motion_tokens",
        "reduced_motion_substitutions",
    ] {
        // assert
        assert!(json.contains(&format!("\"{field}\":")), "missing {field}");
    }
}

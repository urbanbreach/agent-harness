use harness_tui::theme::ColorLevel;
use harness_tui::theme_family::*;

// --- Semantic role coverage ------------------------------------------------

#[test]
fn semantic_role_all_returns_every_role_with_expected_kinds() {
    // arrange
    // act
    let roles = SemanticRole::all();

    // assert
    assert_eq!(roles.len(), 74);
    assert_eq!(
        roles
            .iter()
            .filter(|role| role.kind() == SemanticKind::Palette)
            .count(),
        51
    );
    assert_eq!(
        roles
            .iter()
            .filter(|role| role.kind() == SemanticKind::Glyph)
            .count(),
        14
    );
    assert_eq!(
        roles
            .iter()
            .filter(|role| role.kind() == SemanticKind::Border)
            .count(),
        4
    );
    assert_eq!(
        roles
            .iter()
            .filter(|role| role.kind() == SemanticKind::Focus)
            .count(),
        5
    );
}

#[test]
fn semantic_role_labels_and_display_use_contract_labels() {
    // arrange
    let examples = [
        (SemanticRole::Palette(ColorRole::Canvas), "palette:canvas"),
        (SemanticRole::Glyph(GlyphRole::Streaming), "glyph:streaming"),
        (SemanticRole::Border(BorderRole::None), "border:none"),
        (SemanticRole::Focus(FocusRole::Panel), "focus:panel"),
    ];

    // act
    for (role, expected) in examples {
        // assert
        assert_eq!(role.label(), expected);
        assert_eq!(role.to_string(), expected);
    }
}

// --- Family resolution ------------------------------------------------------

#[test]
fn theme_family_all_and_display_match_contract() {
    // arrange
    // act
    // assert
    assert_eq!(ThemeFamily::all(), [ThemeFamily::Dark, ThemeFamily::Light]);
    assert_eq!(ThemeFamily::Dark.to_string(), "dark");
    assert_eq!(ThemeFamily::Light.to_string(), "light");
}

#[test]
fn theme_family_resolves_every_color_role_for_both_families() {
    // arrange
    // act
    for family in ThemeFamily::all() {
        let resolved = family.resolve_all();
        // assert
        assert_eq!(resolved.len(), ColorRole::ALL.len());
        for &role in &ColorRole::ALL {
            let color = family.resolve(role);
            assert_eq!(
                resolved
                    .iter()
                    .find(|(item, _)| *item == role)
                    .map(|(_, color)| *color),
                Some(color)
            );
        }
    }
    assert_eq!(
        ThemeFamily::Dark.resolve(ColorRole::Canvas).rgb(),
        (20, 20, 20)
    );
}

// --- Fallback ladder ---------------------------------------------------------

#[test]
fn fallback_ladder_resolves_all_levels_and_preserves_determinism() {
    // arrange
    // act
    let rgb = (37, 91, 203);
    let resolved = FallbackLadder::resolve_all(rgb);

    // assert
    assert_eq!(resolved.len(), 4);
    assert_eq!(
        FallbackLadder::resolve(rgb, ColorLevel::TrueColor).rgb(),
        rgb
    );
    assert_eq!(
        FallbackLadder::resolve(rgb, ColorLevel::None).rgb(),
        (0, 0, 0)
    );
    assert_eq!(
        FallbackLadder::resolve(rgb, ColorLevel::Ansi256),
        FallbackLadder::resolve(rgb, ColorLevel::Ansi256)
    );
}

// --- Auto mode detection -----------------------------------------------------

#[test]
fn system_preferences_map_to_theme_families() {
    // arrange
    // act
    // assert
    assert_eq!(SystemPreference::Dark.to_family(), ThemeFamily::Dark);
    assert_eq!(SystemPreference::Light.to_family(), ThemeFamily::Light);
}

#[test]
fn auto_resolver_resolves_and_refreshes_its_cached_preference() {
    // arrange
    let mut resolver = AutoResolver::new();
    assert_eq!(resolver.current(), None);

    let family = resolver.resolve();
    assert!(ThemeFamily::all().contains(&family));
    assert!(resolver.current().is_some());

    // act
    let refreshed = resolver.refresh();
    // assert
    assert_eq!(resolver.current(), Some(refreshed));
}

// --- Preview state machine ---------------------------------------------------

#[test]
fn theme_preview_starts_idle_and_transitions_through_commit() {
    // arrange
    let mut preview = ThemePreview::new(ThemeFamily::Dark);
    assert_eq!(
        preview.state(),
        PreviewState::Idle {
            committed: ThemeFamily::Dark
        }
    );
    assert_eq!(preview.active(), ThemeFamily::Dark);
    assert_eq!(preview.committed(), ThemeFamily::Dark);
    assert!(!preview.is_previewing());

    // act
    preview.begin_preview(ThemeFamily::Light);
    // assert
    assert_eq!(
        preview.state(),
        PreviewState::Previewing {
            prior: ThemeFamily::Dark,
            candidate: ThemeFamily::Light
        }
    );
    assert_eq!(preview.active(), ThemeFamily::Light);
    assert!(preview.is_previewing());
    assert_eq!(preview.commit(), ThemeFamily::Light);
    assert_eq!(
        preview.state(),
        PreviewState::Idle {
            committed: ThemeFamily::Light
        }
    );
}

#[test]
fn theme_preview_cancel_restores_original_prior_across_repeated_previews() {
    // arrange
    // act
    let mut preview = ThemePreview::new(ThemeFamily::Dark);
    preview.begin_preview(ThemeFamily::Light);
    preview.begin_preview(ThemeFamily::Dark);

    // assert
    assert_eq!(preview.committed(), ThemeFamily::Dark);
    assert_eq!(preview.cancel(), ThemeFamily::Dark);
    assert!(!preview.is_previewing());
}

#[test]
fn theme_preview_idle_commit_and_cancel_are_no_ops() {
    // arrange
    // act
    let mut preview = ThemePreview::new(ThemeFamily::Light);
    // assert
    assert_eq!(preview.commit(), ThemeFamily::Light);
    assert_eq!(preview.cancel(), ThemeFamily::Light);
    assert_eq!(
        preview.state(),
        PreviewState::Idle {
            committed: ThemeFamily::Light
        }
    );
}

// --- Persistence round-trip -------------------------------------------------

#[test]
fn theme_choices_parse_map_and_round_trip() {
    // arrange
    assert_eq!(
        ThemeChoice::all(),
        [ThemeChoice::Dark, ThemeChoice::Light, ThemeChoice::Auto]
    );
    assert_eq!(ThemeChoice::from_label("DARK"), Some(ThemeChoice::Dark));
    assert_eq!(ThemeChoice::from_label("light"), Some(ThemeChoice::Light));
    assert_eq!(ThemeChoice::from_label("Auto"), Some(ThemeChoice::Auto));
    assert_eq!(ThemeChoice::from_label("unknown"), None);
    assert_eq!(ThemeChoice::Auto.to_family(true), ThemeFamily::Dark);
    assert_eq!(ThemeChoice::Auto.to_family(false), ThemeFamily::Light);

    // act
    for choice in ThemeChoice::all() {
        let json = serialize_choice(choice).unwrap();
        // assert
        assert_eq!(deserialize_choice(&json).unwrap(), choice);
    }
}

#[test]
fn theme_persistence_rejects_unknown_schema_and_malformed_json() {
    // arrange
    // act
    // assert
    assert!(matches!(
        deserialize_choice(r#"{"schema":"theme-family-v2","theme":"dark"}"#),
        Err(PersistError::UnknownSchema(_))
    ));
    assert!(matches!(
        deserialize_choice("not json"),
        Err(PersistError::Deserialize(_))
    ));
}

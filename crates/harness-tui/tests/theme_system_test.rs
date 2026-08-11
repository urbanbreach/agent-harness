use std::collections::BTreeMap;

use harness_tui::theme::{GlyphMode, Theme};
use harness_tui::theme_system::{
    auto::{detect_system_appearance, SystemAppearance, ThemeChoice, ThemeEnvironment},
    fallback::{ColorLevel, FALLBACK_LADDER},
    family::ThemeFamily,
    persist::{load_theme_choice, store_theme_choice},
    preview::{ThemePreviewState, ThemePreviewStatus},
    roles::{BorderRole, FocusRole, GlyphRole, LifecycleState, Palette, PaletteRole},
};
use ratatui::style::Color;

#[test]
fn legacy_agent_palette_roles_use_generic_accent() {
    // Given: a theme that still carries compatibility slots for historical roles.
    let theme = Theme::default();

    // When: the active theme-system palette resolves those slots.
    let palette = Palette::from_theme(&theme);
    let colors = [
        palette.color(PaletteRole::AgentBuild),
        palette.color(PaletteRole::AgentPlan),
        palette.color(PaletteRole::AgentDocs),
        palette.color(PaletteRole::AgentAsk),
    ];

    // Then: no historical role receives distinct primary chrome.
    assert_eq!(colors, [theme.text.accent; 4]);
}

#[test]
fn theme_family_contract_exposes_every_role_without_missing_mappings() {
    assert_eq!(ThemeFamily::ALL.len(), 5);
    assert_eq!(PaletteRole::ALL.len(), 52);
    assert_eq!(GlyphRole::ALL.len(), 13);
    assert_eq!(BorderRole::ALL.len(), 4);
    assert_eq!(FocusRole::ALL.len(), 5);
    assert_eq!(LifecycleState::ALL.len(), 16);

    for family in ThemeFamily::ALL {
        let resolved = ThemeChoice::explicit(family).resolve(&ThemeEnvironment::default());
        for role in PaletteRole::ALL {
            let _ = resolved.palette.color(role);
        }
        for role in GlyphRole::ALL {
            assert!(!resolved.glyphs.glyph(role).is_empty());
        }
        for role in BorderRole::ALL {
            let _ = resolved.borders.color(role);
        }
        for role in FocusRole::ALL {
            let _ = resolved.focus.style(role);
        }
        for state in LifecycleState::ALL {
            let _ = resolved.bindings.lifecycle.colors(state);
        }
    }
}

#[test]
fn light_and_dark_families_map_every_role_to_truecolor() {
    for family in [ThemeFamily::HarnessDark, ThemeFamily::HarnessLight] {
        let palette = family.palette();
        for role in PaletteRole::ALL {
            assert!(matches!(palette.color(role), Color::Rgb(..)));
        }

        let glyphs = family.glyphs();
        for role in GlyphRole::ALL {
            assert!(!glyphs.glyph(role).is_empty());
        }

        let borders = family.borders();
        for role in BorderRole::ALL {
            match role {
                BorderRole::None => assert_eq!(borders.color(role), Color::Reset),
                BorderRole::Subtle | BorderRole::Strong | BorderRole::Focus => {
                    assert!(matches!(borders.color(role), Color::Rgb(..)))
                }
            }
        }

        for role in FocusRole::ALL {
            let style = family.focus().style(role);
            assert!(matches!(style.foreground, Color::Rgb(..)));
            assert!(matches!(style.background, Color::Rgb(..)));
            assert!(matches!(style.border, Color::Rgb(..)));
        }
    }
}

#[test]
fn family_mapping_uses_theme_tokens_instead_of_raw_color_literals() {
    let source = include_str!("../src/theme_family/family.rs");
    assert!(!source.contains("Color::Rgb"));
    assert!(!source.contains("Color::Indexed"));
    assert!(!source.contains("0x"));
    assert_eq!(ThemeFamily::HarnessDark.theme(), Theme::harness_dark());
    assert_eq!(ThemeFamily::HarnessLight.theme(), Theme::harness_light());
}

#[test]
fn fallback_ladder_has_deterministic_truecolor_to_no_color_matrix() {
    assert_eq!(
        FALLBACK_LADDER,
        [
            ColorLevel::TrueColor,
            ColorLevel::Ansi256,
            ColorLevel::Basic,
            ColorLevel::None,
        ]
    );

    for family in [ThemeFamily::HarnessChat, ThemeFamily::HarnessDark] {
        for level in FALLBACK_LADDER {
            let environment = ThemeEnvironment::with_color_level(level);
            let first = ThemeChoice::explicit(family).resolve(&environment);
            let second = ThemeChoice::explicit(family).resolve(&environment);
            assert_eq!(first, second);
            assert_eq!(first.theme.color_level(), level);
            for role in PaletteRole::ALL {
                let color = first.palette.color(role);
                match level {
                    ColorLevel::TrueColor => assert!(matches!(color, Color::Rgb(..))),
                    ColorLevel::Ansi256 => assert!(matches!(color, Color::Indexed(_))),
                    ColorLevel::Basic | ColorLevel::None => {
                        assert!(!matches!(color, Color::Rgb(..) | Color::Indexed(_)))
                    }
                }
            }
        }
    }
}

#[test]
fn legacy_glyph_mode_uses_semantic_ascii_without_changing_colors() {
    let preferred = Theme::harness_chat();
    let legacy = preferred.with_glyph_mode(GlyphMode::Ascii);

    assert_eq!(legacy.live_shell.glyphs.streaming, "o");
    assert_eq!(legacy.live_shell.glyphs.done, "*");
    assert_eq!(legacy.live_shell.glyphs.error, "x");
    assert_eq!(legacy.live_shell.transcript_glyphs.user_marker, ">");
    assert_eq!(legacy.live_shell.transcript_glyphs.tool_marker, "*");
    assert_eq!(legacy.surface, preferred.surface);
    assert_eq!(legacy.status, preferred.status);
}

#[test]
fn limited_color_modes_preserve_status_meaning() {
    let basic = Theme::harness_chat().for_color_level(ColorLevel::Basic);
    assert_ne!(basic.status.success, basic.status.warning);
    assert_ne!(basic.status.success, basic.status.error);
    assert_ne!(basic.status.warning, basic.status.error);

    let no_color = Theme::harness_chat()
        .for_color_level(ColorLevel::None)
        .with_glyph_mode(GlyphMode::Ascii);
    assert_eq!(no_color.status.success, Color::Reset);
    assert_eq!(no_color.status.warning, Color::Reset);
    assert_eq!(no_color.status.error, Color::Reset);
    assert_ne!(
        no_color.live_shell.glyphs.succeeded,
        no_color.live_shell.glyphs.failed
    );
    assert_ne!(
        no_color.live_shell.glyphs.pending_permission,
        no_color.live_shell.glyphs.running
    );
}

#[test]
fn semantic_bindings_follow_the_theme_token_groups() {
    let resolved =
        ThemeChoice::explicit(ThemeFamily::HarnessDark).resolve(&ThemeEnvironment::default());
    assert_eq!(
        resolved.bindings.selection.background,
        resolved.theme.text.accent
    );
    assert_eq!(
        resolved.bindings.selection.foreground,
        resolved.theme.text.inverse
    );
    assert_eq!(
        resolved.bindings.diff.added_highlight,
        resolved.theme.reference_terminal.diff_added_highlight
    );
    assert_eq!(
        resolved.bindings.diff.removed_highlight,
        resolved.theme.reference_terminal.diff_removed_highlight
    );
    assert_eq!(
        resolved.bindings.tool.pending,
        resolved.theme.status.warning
    );
    assert_eq!(resolved.bindings.tool.failed, resolved.theme.status.error);
    assert_eq!(
        resolved.bindings.permission.surface,
        resolved.theme.surface.panel_elevated
    );
    assert_eq!(
        resolved.bindings.permission.selected,
        resolved.theme.question_prompt.selected
    );
    assert_eq!(
        resolved.bindings.media.placeholder,
        resolved.theme.text.secondary
    );
    assert_eq!(resolved.bindings.media.error, resolved.theme.status.error);
}

#[test]
fn auto_mode_uses_colorfgbg_system_appearance_and_keeps_auto_choice() {
    assert_eq!(
        detect_system_appearance(Some("15;0")),
        Some(SystemAppearance::Dark)
    );
    assert_eq!(
        detect_system_appearance(Some("0;15")),
        Some(SystemAppearance::Light)
    );

    let mut state =
        ThemePreviewState::new(ThemeChoice::Auto, ThemeEnvironment::from_colorfgbg("15;0"));
    assert_eq!(state.effective_theme().family, ThemeFamily::HarnessChat);
    state.on_system_appearance_change(SystemAppearance::Light);
    assert_eq!(state.committed_choice(), ThemeChoice::Auto);
    assert_eq!(state.effective_theme().family, ThemeFamily::HarnessLight);
}

#[test]
fn preview_cancel_restores_the_exact_committed_theme_until_commit() {
    let committed = ThemeChoice::explicit(ThemeFamily::HarnessDark);
    let mut state = ThemePreviewState::new(committed, ThemeEnvironment::default());
    state.preview(ThemeChoice::explicit(ThemeFamily::HarnessLight));
    assert_eq!(state.status(), ThemePreviewStatus::Previewing);
    assert_eq!(state.committed_choice(), committed);
    assert_eq!(state.effective_theme().family, ThemeFamily::HarnessLight);

    state.cancel();
    assert_eq!(state.status(), ThemePreviewStatus::Cancelled);
    assert_eq!(state.committed_choice(), committed);
    assert_eq!(state.effective_theme().family, ThemeFamily::HarnessDark);

    state.preview(ThemeChoice::explicit(ThemeFamily::HarnessLight));
    state.commit();
    assert_eq!(state.status(), ThemePreviewStatus::Committed);
    assert_eq!(
        state.committed_choice(),
        ThemeChoice::explicit(ThemeFamily::HarnessLight)
    );
}

#[test]
fn theme_choice_round_trips_through_existing_tui_keybinds_map() {
    let mut keybinds = BTreeMap::from([(String::from("leader"), String::from("ctrl+x"))]);
    store_theme_choice(&mut keybinds, ThemeChoice::Auto);
    assert_eq!(keybinds.get("leader").map(String::as_str), Some("ctrl+x"));
    assert_eq!(load_theme_choice(&keybinds), Ok(ThemeChoice::Auto));

    store_theme_choice(
        &mut keybinds,
        ThemeChoice::explicit(ThemeFamily::HarnessLight),
    );
    assert_eq!(
        load_theme_choice(&keybinds),
        Ok(ThemeChoice::explicit(ThemeFamily::HarnessLight))
    );
}

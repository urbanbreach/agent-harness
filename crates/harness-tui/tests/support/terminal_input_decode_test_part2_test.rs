#[test]
fn brand_detection_by_term_program() {
    // arrange
    // act
    // assert
    assert_eq!(
        TerminalName::detect(&env_term_program("ghostty")),
        TerminalName::Ghostty
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("WarpTerminal")),
        TerminalName::WarpTerminal
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("vscode")),
        TerminalName::VsCode
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("Zed")),
        TerminalName::Zed
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("WezTerm")),
        TerminalName::WezTerm
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("iTerm.app")),
        TerminalName::Iterm2
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("Apple_Terminal")),
        TerminalName::AppleTerminal
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("rio")),
        TerminalName::Rio
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("kitty")),
        TerminalName::Kitty
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("grok-desktop")),
        TerminalName::GrokDesktop
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("otty")),
        TerminalName::Otty
    );
}

#[test]
fn brand_detection_by_specific_env_markers() {
    // arrange
    let kitty = TerminalEnv {
        kitty_window_id: Some("1".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&kitty), TerminalName::Kitty);

    let ghostty = TerminalEnv {
        ghostty_resources_dir: Some("/share".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&ghostty), TerminalName::Ghostty);

    let warp = TerminalEnv {
        warp_session_id: Some("s".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&warp), TerminalName::WarpTerminal);

    let terminator = TerminalEnv {
        terminator_uuid: Some("urn:uuid".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&terminator), TerminalName::Terminator);

    let windows_terminal = TerminalEnv {
        wt_session: Some("{guid}".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalName::detect(&windows_terminal),
        TerminalName::WindowsTerminal
    );

    // act
    let jetbrains = TerminalEnv {
        terminal_emulator: Some("JetBrains-JediTerm".to_string()),
        ..TerminalEnv::default()
    };
    // assert
    assert_eq!(TerminalName::detect(&jetbrains), TerminalName::JetBrains);
}

#[test]
fn brand_detection_by_term_value() {
    // arrange
    let alacritty = TerminalEnv {
        term: Some("alacritty".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&alacritty), TerminalName::Alacritty);

    let foot = TerminalEnv {
        term: Some("foot".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&foot), TerminalName::Foot);

    // act
    let vte = TerminalEnv {
        vte_version: Some("7400".to_string()),
        ..TerminalEnv::default()
    };
    // assert
    assert_eq!(TerminalName::detect(&vte), TerminalName::Vte);
}

#[test]
fn vscode_family_forks_discriminate_over_vscode() {
    // arrange
    let cursor = TerminalEnv {
        term_program: Some("vscode".to_string()),
        cursor_session: Some("sess".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&cursor), TerminalName::Cursor);

    // act
    let windsurf = TerminalEnv {
        term_program: Some("vscode".to_string()),
        windsurf: Some("1".to_string()),
        ..TerminalEnv::default()
    };
    // assert
    assert_eq!(TerminalName::detect(&windsurf), TerminalName::Windsurf);
}

#[test]
fn lc_terminal_marker_detects_iterm2() {
    // arrange
    // act
    let iterm = TerminalEnv {
        lc_terminal: Some("iTerm2".to_string()),
        ..TerminalEnv::default()
    };
    // assert
    assert_eq!(TerminalName::detect(&iterm), TerminalName::Iterm2);
}

// ---------------------------------------------------------------------------
// P3 — brand capability conditionals
// ---------------------------------------------------------------------------

#[test]
fn vte_based_conditionals_select_expected_protocol() {
    // arrange
    // act
    // assert
    assert!(TerminalName::Vte.is_vte_based());
    assert!(TerminalName::Terminator.is_vte_based());
    assert!(!TerminalName::Ghostty.is_vte_based());
    assert!(!TerminalName::Kitty.is_vte_based());
}

#[test]
fn vscode_family_conditionals_select_expected_protocol() {
    // arrange
    // act
    for brand in [
        TerminalName::VsCode,
        TerminalName::Cursor,
        TerminalName::Windsurf,
    ] {
        // assert
        assert!(brand.is_vscode_family(), "{brand:?} expected vscode family");
    }
    assert!(!TerminalName::Ghostty.is_vscode_family());
}

#[test]
fn unclassified_capability_conditionals_select_safe_defaults() {
    // arrange
    // act
    // assert
    assert!(TerminalName::Unknown.is_capability_unclassified());
    assert!(TerminalName::Otty.is_capability_unclassified());
    assert!(!TerminalName::Ghostty.is_capability_unclassified());
    assert!(!TerminalName::VsCode.is_capability_unclassified());
}

#[test]
fn osc52_support_is_brand_allowlisted() {
    // arrange
    // act
    // assert
    assert!(TerminalName::Ghostty.supports_osc52_clipboard());
    assert!(TerminalName::Kitty.supports_osc52_clipboard());
    assert!(TerminalName::WezTerm.supports_osc52_clipboard());
    assert!(TerminalName::Cursor.supports_osc52_clipboard());
    assert!(!TerminalName::AppleTerminal.supports_osc52_clipboard());
    assert!(!TerminalName::Vte.supports_osc52_clipboard());
    assert!(!TerminalName::Unknown.supports_osc52_clipboard());
}

#[test]
fn csi_query_interception_conditionals() {
    // arrange
    // act
    // assert
    assert!(TerminalName::JetBrains.intercepts_csi_queries());
    assert!(TerminalName::WarpTerminal.intercepts_csi_queries());
    assert!(!TerminalName::Ghostty.intercepts_csi_queries());
}

#[test]
fn enhanced_keyboard_support_conditionals() {
    // arrange
    // act
    // assert
    assert!(TerminalName::Kitty.supports_enhanced_keyboard());
    assert!(TerminalName::Ghostty.supports_enhanced_keyboard());
    assert!(TerminalName::WindowsTerminal.supports_enhanced_keyboard());
    assert!(!TerminalName::AppleTerminal.supports_enhanced_keyboard());
    assert!(!TerminalName::Vte.supports_enhanced_keyboard());
    assert!(!TerminalName::Unknown.supports_enhanced_keyboard());
}

#[test]
fn shift_enter_diverges_from_generic_enhanced_keyboard() {
    // arrange
    // act
    // WindowsTerminal has the enhanced protocol yet still mishandles Shift+Enter.
    // assert
    assert!(TerminalName::WindowsTerminal.supports_enhanced_keyboard());
    let ctx = TerminalContext {
        brand: TerminalName::WindowsTerminal,
        multiplexer: TerminalMultiplexer::Undetected,
        alt_screen: harness_tui::terminal::lifecycle::AltScreenMode::Auto,
        is_tty: true,
        is_byobu: false,
    };
    assert!(ctx.shift_enter_unavailable());
}

// ---------------------------------------------------------------------------
// P3 — multiplexer detection and conditionals
// ---------------------------------------------------------------------------

#[test]
fn multiplexer_detection_identifies_supported_hosts() {
    // arrange
    let tmux = TerminalEnv {
        tmux: Some("/tmp/tmux-1000/default,123,0".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalMultiplexer::detect(&tmux),
        TerminalMultiplexer::Tmux
    );

    let zellij = TerminalEnv {
        zellij: Some("1".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalMultiplexer::detect(&zellij),
        TerminalMultiplexer::Zellij
    );

    let screen = TerminalEnv {
        screen_sty: Some("1234.pts-0.host".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalMultiplexer::detect(&screen),
        TerminalMultiplexer::Screen
    );

    // act
    let cmux = TerminalEnv {
        cmux: Some("1".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalMultiplexer::detect(&cmux),
        TerminalMultiplexer::Cmux
    );

    // assert
    assert_eq!(
        TerminalMultiplexer::detect(&TerminalEnv::default()),
        TerminalMultiplexer::Undetected
    );
}

#[test]
fn byobu_detection_defaults_to_tmux_backend() {
    // arrange
    let byobu = TerminalEnv {
        byobu: Some("1".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalMultiplexer::detect(&byobu),
        TerminalMultiplexer::Tmux
    );

    // act
    let byobu_screen = TerminalEnv {
        byobu: Some("1".to_string()),
        byobu_backend: Some("screen".to_string()),
        ..TerminalEnv::default()
    };
    // assert
    assert_eq!(
        TerminalMultiplexer::detect(&byobu_screen),
        TerminalMultiplexer::Screen
    );
}

#[test]
fn multiplexer_csi_interception_preserves_terminal_input() {
    // arrange
    // act
    // assert
    assert!(TerminalMultiplexer::Tmux.intercepts_csi_queries());
    assert!(TerminalMultiplexer::Screen.intercepts_csi_queries());
    assert!(TerminalMultiplexer::Zellij.intercepts_csi_queries());
    assert!(!TerminalMultiplexer::Undetected.intercepts_csi_queries());
}

// ---------------------------------------------------------------------------
// P3 / P6 — capability fallback / graceful degradation
// ---------------------------------------------------------------------------

fn ghostty_env() -> TerminalEnv {
    env_term_program("ghostty")
}

fn apple_env() -> TerminalEnv {
    env_term_program("Apple_Terminal")
}

#[test]
fn modern_terminal_resolves_full_capabilities() {
    // arrange
    // act
    let caps = terminal_capability_fallback(&ghostty_env(), ColorMode::Truecolor, true);
    let expected = TerminalCapabilityLeaf {
        color_mode: ColorMode::Truecolor,
        keyboard_mode: harness_tui::terminal::KeyboardMode::Enhanced,
        mouse_capture: true,
        bracketed_paste: true,
        osc52_clipboard: true,
        alternate_screen: true,
        focus_reporting: true,
    };
    // assert
    assert_eq!(caps, expected);
}

#[test]
fn apple_terminal_degrades_mouse_and_keyboard() {
    // arrange
    // act
    let ctx = TerminalContext::probe(&apple_env(), true);
    // assert
    assert_eq!(ctx.brand, TerminalName::AppleTerminal);
    let caps = ctx.resolve(ColorMode::Ansi256);
    assert!(
        !caps.mouse_capture,
        "mouse leaks as raw text on Apple Terminal"
    );
    assert!(
        !caps.osc52_clipboard,
        "Apple Terminal is not OSC52-allowlisted"
    );
    assert_eq!(
        caps.keyboard_mode,
        harness_tui::terminal::KeyboardMode::Legacy
    );
    assert!(
        caps.bracketed_paste,
        "bracketed paste still available on a TTY"
    );
}

#[test]
fn mouse_leak_brands_disable_mouse_capture() {
    // arrange
    // act
    for brand in [
        TerminalName::Unknown,
        TerminalName::Otty,
        TerminalName::JetBrains,
    ] {
        let ctx = TerminalContext {
            brand,
            multiplexer: TerminalMultiplexer::Undetected,
            alt_screen: harness_tui::terminal::lifecycle::AltScreenMode::Auto,
            is_tty: true,
            is_byobu: false,
        };
        // assert
        assert!(
            ctx.mouse_reporting_leaks_as_raw_text(),
            "{brand:?} should leak"
        );
        assert!(
            !ctx.resolve(ColorMode::Ansi256).mouse_capture,
            "{brand:?} must disable capture"
        );
    }
}

#[test]
fn multiplexer_disables_focus_reporting_and_auto_alt_screen() {
    // arrange
    // act
    let env = TerminalEnv {
        term_program: Some("ghostty".to_string()),
        tmux: Some("/tmp/tmux-1000/default".to_string()),
        ..TerminalEnv::default()
    };
    let ctx = TerminalContext::probe(&env, true);
    // assert
    assert!(ctx.repaints_pane_out_of_band(), "tmux repaints out of band");
    let caps = ctx.resolve(ColorMode::Truecolor);
    assert!(
        !caps.focus_reporting,
        "focus reporting degraded under multiplexer"
    );
    assert!(
        !caps.alternate_screen,
        "Auto alt-screen disabled under multiplexer"
    );
}

#[test]
fn alt_screen_always_engages_even_under_multiplexer() {
    // arrange
    let env = TerminalEnv {
        term_program: Some("ghostty".to_string()),
        tmux: Some("/tmp/tmux".to_string()),
        ..TerminalEnv::default()
    };
    let mut ctx = TerminalContext::probe(&env, true);
    ctx.alt_screen = harness_tui::terminal::lifecycle::AltScreenMode::Always;
    assert!(ctx.resolve(ColorMode::Truecolor).alternate_screen);

    // act
    ctx.alt_screen = harness_tui::terminal::lifecycle::AltScreenMode::Never;
    // assert
    assert!(!ctx.resolve(ColorMode::Truecolor).alternate_screen);
}

#[test]
fn non_tty_disables_interactive_features() {
    // arrange
    // act
    let caps = terminal_capability_fallback(&ghostty_env(), ColorMode::Truecolor, false);
    // assert
    assert!(!caps.mouse_capture);
    assert!(!caps.bracketed_paste);
    assert!(!caps.osc52_clipboard);
    assert!(!caps.alternate_screen);
    assert!(!caps.focus_reporting);
}

#[test]
fn ctrl_dot_unreliable_for_vscode_family_and_apple() {
    // arrange
    // act
    for brand in [
        TerminalName::VsCode,
        TerminalName::Cursor,
        TerminalName::Windsurf,
        TerminalName::AppleTerminal,
    ] {
        let ctx = TerminalContext::probe(
            &TerminalEnv {
                term_program: None,
                ..TerminalEnv::default()
            },
            true,
        );
        let ctx = TerminalContext { brand, ..ctx };
        // assert
        assert!(
            ctx.ctrl_dot_unreliable(),
            "{brand:?} expected unreliable Ctrl+."
        );
    }
    let ghostty = TerminalContext::probe(&ghostty_env(), true);
    assert!(!ghostty.ctrl_dot_unreliable());
}

#[test]
fn csi_queries_unavailable_under_interceptor_or_multiplexer() {
    // arrange
    let jetbrains = TerminalContext::probe(
        &TerminalEnv {
            terminal_emulator: Some("JetBrains-JediTerm".to_string()),
            ..TerminalEnv::default()
        },
        true,
    );
    assert!(!jetbrains.csi_queries_available());

    let tmux = TerminalContext::probe(
        &TerminalEnv {
            term_program: Some("ghostty".to_string()),
            tmux: Some("/tmp/tmux".to_string()),
            ..TerminalEnv::default()
        },
        true,
    );
    assert!(!tmux.csi_queries_available());

    // act
    let ghostty = TerminalContext::probe(&ghostty_env(), true);
    // assert
    assert!(ghostty.csi_queries_available());
}

#[test]
fn ime_as_bracketed_paste_conditional_is_brand_specific() {
    // arrange
    // act
    // assert
    assert!(TerminalName::Vte.delivers_ime_as_bracketed_paste());
    assert!(TerminalName::AppleTerminal.delivers_ime_as_bracketed_paste());
    assert!(TerminalName::JetBrains.delivers_ime_as_bracketed_paste());
    assert!(!TerminalName::Ghostty.delivers_ime_as_bracketed_paste());
    assert!(!TerminalName::Kitty.delivers_ime_as_bracketed_paste());
}

#[test]
fn probe_records_byobu_flag() {
    // arrange
    // act
    let env = TerminalEnv {
        byobu: Some("1".to_string()),
        ..TerminalEnv::default()
    };
    let ctx = TerminalContext::probe(&env, true);
    // assert
    assert!(ctx.byobu());
    assert!(ctx.is_tmux_backed(), "byobu defaults to a tmux backend");
}

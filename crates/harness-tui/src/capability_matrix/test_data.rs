use crate::capability_matrix::CapabilityClassifier;

pub fn well_known_profiles() -> Vec<(&'static str, CapabilityClassifier)> {
    let profile = |term: &str, program: &str, color: &str, tmux: bool| {
        CapabilityClassifier::new(
            term.into(),
            program.into(),
            color.into(),
            tmux,
            false,
            false,
            false,
            false,
            None,
        )
    };
    vec![
        (
            "wezterm-no-mux",
            profile("xterm-256color", "WezTerm", "truecolor", false),
        ),
        ("kitty", profile("xterm-kitty", "kitty", "truecolor", false)),
        ("iterm2", profile("xterm-256color", "iTerm.app", "", false)),
        ("xterm-256", profile("xterm-256color", "", "", false)),
        ("dumb", profile("dumb", "", "", false)),
        ("tmux", profile("screen-256color", "", "", true)),
    ]
}

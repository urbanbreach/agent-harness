use harness_testkit::tui_fidelity::{
    CheckpointName, IdentityScope, IdentitySubstitution, Rgb, Scenario, TextPlacement, TextStyle,
    Viewport, Wrapping,
};

pub(super) fn add(scenario: &mut Scenario, viewport: Viewport) {
    if !scenario.substitutions.is_empty() || viewport.cols < 12 {
        return;
    }
    for checkpoint in [
        CheckpointName::Rest,
        CheckpointName::Mid,
        CheckpointName::Settled,
    ] {
        scenario.substitutions.push(IdentitySubstitution {
            checkpoint,
            scope: IdentityScope::WorkspacePath,
            rectangle: harness_testkit::tui_fidelity::CellRect {
                col: 2,
                row: 1,
                cols: viewport.cols - 2,
                rows: 1,
            },
            source: workspace_source(viewport.cols - 2),
            target: workspace_target(viewport.cols - 2),
        });
        if viewport.rows > 26 && viewport.cols >= 51 {
            scenario.substitutions.push(IdentitySubstitution {
                checkpoint,
                scope: IdentityScope::ProviderName,
                rectangle: harness_testkit::tui_fidelity::CellRect {
                    col: viewport.cols - 51,
                    row: 26,
                    cols: 46,
                    rows: 1,
                },
                source: provider_source(),
                target: provider_target(),
            });
        }
    }
}

fn identity_style(dim: bool) -> TextStyle {
    TextStyle {
        foreground: Rgb {
            r: 216,
            g: 216,
            b: 216,
        },
        background: Rgb {
            r: 18,
            g: 18,
            b: 18,
        },
        bold: false,
        dim,
        italic: false,
        underline: false,
        inverse: false,
    }
}

fn workspace_source(width: u16) -> TextPlacement {
    TextPlacement {
        text: "<harness-workspace>".to_owned(),
        cell_width: width,
        padding_left: 0,
        padding_right: 0,
        style: identity_style(true),
        wrapping: Wrapping::NoWrap,
    }
}

fn workspace_target(width: u16) -> TextPlacement {
    TextPlacement {
        text: IdentityScope::WorkspacePath.placeholder().to_owned(),
        cell_width: 10,
        padding_left: 0,
        padding_right: width - 10,
        style: identity_style(true),
        wrapping: Wrapping::NoWrap,
    }
}

fn provider_source() -> TextPlacement {
    TextPlacement {
        text: "GPT 5.6 Luna (CLIProxy) (low) · always-approve".to_owned(),
        cell_width: 46,
        padding_left: 0,
        padding_right: 0,
        style: identity_style(false),
        wrapping: Wrapping::NoWrap,
    }
}

fn provider_target() -> TextPlacement {
    TextPlacement {
        text: IdentityScope::ProviderName.placeholder().to_owned(),
        cell_width: 10,
        padding_left: 0,
        padding_right: 36,
        style: identity_style(false),
        wrapping: Wrapping::NoWrap,
    }
}

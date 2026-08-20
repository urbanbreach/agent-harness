use harness_testkit::tui_fidelity::{
    CheckpointName, Rgb, Scenario, SubstitutionField, SubstitutionKind, TextPlacement, TextStyle,
    TextSubstitution, Viewport, Wrapping,
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
        scenario.substitutions.push(TextSubstitution {
            checkpoint,
            kind: SubstitutionKind::TruthfulDynamicText,
            field: SubstitutionField::WorkspacePath,
            canonical_placeholder: SubstitutionField::WorkspacePath.placeholder().to_owned(),
            reference_provenance: "reference-runtime:workspace-header".to_owned(),
            candidate_provenance: "candidate-runtime:workspace-header".to_owned(),
            rectangle: harness_testkit::tui_fidelity::CellRect {
                col: 2,
                row: 1,
                cols: 19,
                rows: 1,
            },
            reference: workspace_reference(),
            candidate: workspace_candidate(),
        });
        if viewport.rows > 26 && viewport.cols >= 51 {
            scenario.substitutions.push(TextSubstitution {
                checkpoint,
                kind: SubstitutionKind::TruthfulDynamicText,
                field: SubstitutionField::ProviderName,
                canonical_placeholder: SubstitutionField::ProviderName.placeholder().to_owned(),
                reference_provenance: "reference-runtime:status-provider".to_owned(),
                candidate_provenance: "candidate-runtime:status-provider".to_owned(),
                rectangle: harness_testkit::tui_fidelity::CellRect {
                    col: viewport.cols - 51,
                    row: viewport.rows - 4,
                    cols: 46,
                    rows: 1,
                },
                reference: provider_reference(),
                candidate: provider_candidate(),
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

fn workspace_reference() -> TextPlacement {
    TextPlacement {
        text: "<grok-workspace>".to_owned(),
        cell_width: 16,
        padding_left: 0,
        padding_right: 3,
        style: identity_style(true),
        wrapping: Wrapping::NoWrap,
    }
}

fn workspace_candidate() -> TextPlacement {
    TextPlacement {
        text: "<harness-workspace>".to_owned(),
        cell_width: 19,
        padding_left: 0,
        padding_right: 0,
        style: identity_style(true),
        wrapping: Wrapping::NoWrap,
    }
}

fn provider_reference() -> TextPlacement {
    TextPlacement {
        text: "GPT 5.6 Luna (CLIProxy) (low) · always-approve".to_owned(),
        cell_width: 46,
        padding_left: 0,
        padding_right: 0,
        style: identity_style(false),
        wrapping: Wrapping::NoWrap,
    }
}

fn provider_candidate() -> TextPlacement {
    TextPlacement {
        text: "Harness Demo provider".to_owned(),
        cell_width: 21,
        padding_left: 25,
        padding_right: 0,
        style: identity_style(false),
        wrapping: Wrapping::NoWrap,
    }
}

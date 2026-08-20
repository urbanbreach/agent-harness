use crate::parity::{CellModifiers, CursorState, ResolvedRgb, SemanticCell, SemanticFrame};
use crate::tui_fidelity::{CheckpointName, Scenario, TextPlacement, TextSubstitution};

use super::super::types::CellSnapshot;
use super::semantic_pixels::verified_masks_for;

const IDLE: &str = include_str!("../../tui_fidelity_scenarios/baseline/idle.json");
const STARTUP: &str = include_str!("../../tui_fidelity_scenarios/baseline/startup.json");
const CURRENT_PROVIDER_PLACEMENT: &str = "                             model-1 · Demo mode";
const STALE_PROVIDER_PLACEMENT: &str = "                                 model-1 · Demo";

#[test]
fn visually_similar_undeclared_dynamic_text_is_not_masked() -> Result<(), Box<dyn std::error::Error>>
{
    // arrange
    let scenario = Scenario::from_json(IDLE)?;
    let expected = snapshot("~/.grok")?;
    let actual = snapshot("~/.harness")?;
    let masks = verified_masks_for(
        &scenario,
        CheckpointName::Rest,
        &expected.frame,
        &actual.frame,
    )?;

    // act
    let result = super::super::cells::compare_cells(&expected, &actual, &masks);

    // assert
    assert!(result.is_err());
    Ok(())
}

#[test]
fn declared_values_are_verified_before_typed_mask_is_created(
) -> Result<(), Box<dyn std::error::Error>> {
    // arrange
    let scenario = home_scenario()?;
    let substitution = home_substitution(&scenario)?;
    let reference = placement_frame(substitution, &substitution.reference)?;
    let candidate = placement_frame(substitution, &substitution.candidate)?;

    // act
    let masks = verified_masks_for(&scenario, CheckpointName::Mid, &reference, &candidate)?;

    // assert
    assert_eq!(
        masks.grapheme_mask_field(16, 13),
        Some("truthful_dynamic_text:home_path")
    );
    Ok(())
}

#[test]
fn arbitrary_candidate_mutation_inside_declared_rectangle_is_rejected(
) -> Result<(), Box<dyn std::error::Error>> {
    // arrange
    let scenario = home_scenario()?;
    let substitution = home_substitution(&scenario)?;
    let reference = placement_frame(substitution, &substitution.reference)?;
    let mut candidate = placement_frame(substitution, &substitution.candidate)?;
    candidate.set_cell(styled_cell(
        substitution.rectangle.row,
        substitution.rectangle.col,
        "X",
        substitution,
    ))?;

    // act
    let result = verified_masks_for(&scenario, CheckpointName::Mid, &reference, &candidate);

    // assert
    assert!(result.is_err());
    assert!(result
        .expect_err("mutated declared value must fail")
        .to_string()
        .contains("does not equal declared value"));
    Ok(())
}

#[test]
fn current_startup_provider_placement_is_verified_before_masking(
) -> Result<(), Box<dyn std::error::Error>> {
    // arrange: the captured runtime field uses the current startup-only mode badge.
    let scenario = provider_scenario()?;

    // act: the comparator verifies both placements before constructing the mask.
    let placements = [
        CheckpointName::Rest,
        CheckpointName::Mid,
        CheckpointName::Settled,
    ]
    .into_iter()
    .map(|checkpoint| {
        let substitution = provider_substitution(&scenario, checkpoint)?;
        let reference = placement_frame(substitution, &substitution.reference)?;
        let candidate = placement_frame(substitution, &substitution.candidate)?;
        let masks = verified_masks_for(&scenario, checkpoint, &reference, &candidate)?;
        Ok::<_, Box<dyn std::error::Error>>((
            checkpoint,
            substitution.rectangle.cols,
            substitution.candidate.text.as_str(),
            substitution.candidate.cell_width,
            substitution.candidate.padding_left,
            substitution.candidate.padding_right,
            masks.grapheme_mask_field(26, 46) == Some("truthful_dynamic_text:provider_name"),
        ))
    })
    .collect::<Result<Vec<_>, _>>()?;

    // assert: the exact provider field is the only accepted typed span.
    assert_eq!(
        placements,
        [
            CheckpointName::Rest,
            CheckpointName::Mid,
            CheckpointName::Settled,
        ]
        .into_iter()
        .map(|checkpoint| (checkpoint, 49, CURRENT_PROVIDER_PLACEMENT, 48, 1, 0, true,))
        .collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn stale_startup_provider_value_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    // arrange: the declaration regresses to Demo while captured cells retain Demo mode.
    let mut scenario = provider_scenario()?;
    scenario.substitutions[0].candidate.text = STALE_PROVIDER_PLACEMENT.to_owned();
    let substitution = provider_substitution(&scenario, CheckpointName::Rest)?;
    let reference = placement_frame(substitution, &substitution.reference)?;
    let mut runtime = substitution.candidate.clone();
    runtime.text = CURRENT_PROVIDER_PLACEMENT.to_owned();
    let candidate = placement_frame(substitution, &runtime)?;

    // act: the stale declaration crosses the exact placement verification boundary.
    let result = verified_masks_for(&scenario, CheckpointName::Rest, &reference, &candidate);

    // assert: no mask is created for a candidate value that differs from captured cells.
    assert!(result
        .expect_err("stale provider declaration must fail")
        .to_string()
        .contains("does not equal declared value"));
    Ok(())
}

fn home_substitution(scenario: &Scenario) -> Result<&TextSubstitution, Box<dyn std::error::Error>> {
    scenario
        .substitutions
        .iter()
        .find(|item| item.checkpoint == CheckpointName::Mid && item.field.as_str() == "home_path")
        .ok_or_else(|| "mid home substitution is missing".into())
}

fn home_scenario() -> Result<Scenario, Box<dyn std::error::Error>> {
    let mut scenario = Scenario::from_json(STARTUP)?;
    scenario.substitutions.retain(|item| {
        item.checkpoint == CheckpointName::Mid && item.field.as_str() == "home_path"
    });
    Ok(scenario)
}

fn provider_substitution(
    scenario: &Scenario,
    checkpoint: CheckpointName,
) -> Result<&TextSubstitution, Box<dyn std::error::Error>> {
    scenario
        .substitutions
        .iter()
        .find(|item| item.checkpoint == checkpoint && item.field.as_str() == "provider_name")
        .ok_or_else(|| "rest provider substitution is missing".into())
}

fn provider_scenario() -> Result<Scenario, Box<dyn std::error::Error>> {
    let mut scenario = Scenario::from_json(STARTUP)?;
    scenario
        .substitutions
        .retain(|item| item.field.as_str() == "provider_name");
    Ok(scenario)
}

fn placement_frame(
    substitution: &TextSubstitution,
    placement: &TextPlacement,
) -> Result<SemanticFrame, Box<dyn std::error::Error>> {
    let mut frame = SemanticFrame::new(100, 30, CursorState::hidden(0, 0));
    for col in substitution.rectangle.col
        ..substitution
            .rectangle
            .col
            .saturating_add(substitution.rectangle.cols)
    {
        frame.set_cell(styled_cell(
            substitution.rectangle.row,
            col,
            "",
            substitution,
        ))?;
    }
    for (offset, character) in placement.text.chars().enumerate() {
        let col = substitution
            .rectangle
            .col
            .saturating_add(placement.padding_left)
            .saturating_add(u16::try_from(offset)?);
        frame.set_cell(styled_cell(
            substitution.rectangle.row,
            col,
            &character.to_string(),
            substitution,
        ))?;
    }
    Ok(frame)
}

fn styled_cell(
    row: u16,
    col: u16,
    grapheme: &str,
    substitution: &TextSubstitution,
) -> SemanticCell {
    let style = substitution.reference.style;
    SemanticCell::blank(row, col)
        .with_grapheme(grapheme, 1)
        .with_fg(ResolvedRgb::new(
            style.foreground.r,
            style.foreground.g,
            style.foreground.b,
        ))
        .with_bg(ResolvedRgb::new(
            style.background.r,
            style.background.g,
            style.background.b,
        ))
        .with_modifiers(CellModifiers {
            bold: style.bold,
            dim: style.dim,
            italic: style.italic,
            underline: style.underline,
            inverse: style.inverse,
        })
}

fn snapshot(text: &str) -> Result<CellSnapshot, Box<dyn std::error::Error>> {
    let mut frame = SemanticFrame::new(100, 30, CursorState::hidden(0, 0));
    frame.set_cell(SemanticCell::blank(0, 0).with_grapheme(text.to_owned(), 1))?;
    Ok(CellSnapshot {
        frame,
        focus: None,
        z_order: Vec::new(),
    })
}

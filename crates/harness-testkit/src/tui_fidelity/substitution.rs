use super::error::{ScenarioError, SubstitutionError};
use super::types::{CellRect, Checkpoint, IdentitySubstitution, TextPlacement, Viewport, Wrapping};

pub(super) fn validate_substitutions(
    substitutions: &[IdentitySubstitution],
    checkpoints: &[Checkpoint],
) -> Result<(), ScenarioError> {
    let mut seen_scopes = Vec::new();
    for substitution in substitutions {
        let Some(checkpoint) = checkpoints
            .iter()
            .find(|checkpoint| checkpoint.name == substitution.checkpoint)
        else {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::RectangleGeometry,
            ));
        };
        if seen_scopes.contains(&(substitution.checkpoint, substitution.scope)) {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::DuplicateScope {
                    checkpoint: substitution.checkpoint,
                },
            ));
        }
        seen_scopes.push((substitution.checkpoint, substitution.scope));
        validate_rectangle(substitution.rectangle, checkpoint.frame.viewport)?;
        validate_placement(&substitution.source, substitution.rectangle)?;
        validate_placement(&substitution.target, substitution.rectangle)?;
        if substitution.source.text.is_empty() || substitution.target.text.is_empty() {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::EmptyText,
            ));
        }
        if substitution.source.text.chars().any(char::is_control)
            || substitution.target.text.chars().any(char::is_control)
        {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::ControlText,
            ));
        }
        if substitution.source.text == substitution.target.text {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::SameText,
            ));
        }
        if substitution.target.text != substitution.scope.placeholder() {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::NonIdentityReplacement,
            ));
        }
        if substitution.source.style != substitution.target.style {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::StyleMismatch,
            ));
        }
        if substitution.source.wrapping != substitution.target.wrapping {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::WrappingMismatch,
            ));
        }
    }
    Ok(())
}

fn validate_rectangle(rectangle: CellRect, viewport: Viewport) -> Result<(), ScenarioError> {
    if rectangle.cols == 0
        || rectangle.rows == 0
        || u32::from(rectangle.col) + u32::from(rectangle.cols) > u32::from(viewport.cols)
        || u32::from(rectangle.row) + u32::from(rectangle.rows) > u32::from(viewport.rows)
    {
        return Err(ScenarioError::InvalidSubstitution(
            SubstitutionError::RectangleGeometry,
        ));
    }
    let area = u32::from(rectangle.cols) * u32::from(rectangle.rows);
    let viewport_area = u32::from(viewport.cols) * u32::from(viewport.rows);
    if rectangle.cols == viewport.cols
        || rectangle.rows == viewport.rows
        || area.saturating_mul(4) >= viewport_area
    {
        return Err(ScenarioError::InvalidSubstitution(
            SubstitutionError::BroadRegion,
        ));
    }
    Ok(())
}

fn validate_placement(placement: &TextPlacement, rectangle: CellRect) -> Result<(), ScenarioError> {
    let width = u32::from(placement.cell_width)
        + u32::from(placement.padding_left)
        + u32::from(placement.padding_right);
    if placement.cell_width == 0
        || width != u32::from(rectangle.cols)
        || matches!(placement.wrapping, Wrapping::NoWrap) && rectangle.rows != 1
    {
        return Err(ScenarioError::InvalidSubstitution(
            SubstitutionError::PaddingMismatch,
        ));
    }
    Ok(())
}

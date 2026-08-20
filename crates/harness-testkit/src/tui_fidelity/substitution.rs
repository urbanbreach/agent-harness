use super::error::{ScenarioError, SubstitutionError};
use super::types::{CellRect, Checkpoint, TextPlacement, TextSubstitution, Viewport, Wrapping};

pub(super) fn validate_substitutions(
    substitutions: &[TextSubstitution],
    checkpoints: &[Checkpoint],
) -> Result<(), ScenarioError> {
    let mut seen_fields = Vec::new();
    for substitution in substitutions {
        let Some(checkpoint) = checkpoints
            .iter()
            .find(|checkpoint| checkpoint.name == substitution.checkpoint)
        else {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::RectangleGeometry,
            ));
        };
        if seen_fields.contains(&(substitution.checkpoint, substitution.field)) {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::DuplicateField {
                    checkpoint: substitution.checkpoint,
                },
            ));
        }
        seen_fields.push((substitution.checkpoint, substitution.field));
        if !substitution.field.permits(substitution.kind) {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::FieldKindMismatch,
            ));
        }
        if substitution.reference_provenance.trim().is_empty() {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::MissingReferenceProvenance,
            ));
        }
        if substitution.candidate_provenance.trim().is_empty() {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::MissingCandidateProvenance,
            ));
        }
        if substitution
            .reference_provenance
            .chars()
            .any(char::is_control)
        {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::ControlReferenceProvenance,
            ));
        }
        if substitution
            .candidate_provenance
            .chars()
            .any(char::is_control)
        {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::ControlCandidateProvenance,
            ));
        }
        validate_rectangle(substitution.rectangle, checkpoint.frame.viewport)?;
        validate_placement(&substitution.reference, substitution.rectangle)?;
        validate_placement(&substitution.candidate, substitution.rectangle)?;
        if substitution.reference.text.is_empty() || substitution.candidate.text.is_empty() {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::EmptyText,
            ));
        }
        if substitution
            .reference
            .text
            .chars()
            .any(invalid_text_control)
            || substitution
                .candidate
                .text
                .chars()
                .any(invalid_text_control)
        {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::ControlText,
            ));
        }
        if substitution.reference.text == substitution.candidate.text {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::SameText,
            ));
        }
        if substitution.canonical_placeholder != substitution.field.placeholder() {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::NonCanonicalPlaceholder,
            ));
        }
        if substitution.reference.style != substitution.candidate.style {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::StyleMismatch,
            ));
        }
        if substitution.reference.wrapping != substitution.candidate.wrapping {
            return Err(ScenarioError::InvalidSubstitution(
                SubstitutionError::WrappingMismatch,
            ));
        }
    }
    Ok(())
}

fn invalid_text_control(character: char) -> bool {
    character.is_control() && character != '\n'
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
        || u32::from(rectangle.cols).saturating_mul(4) >= u32::from(viewport.cols).saturating_mul(3)
        || area.saturating_mul(4) >= viewport_area
    {
        return Err(ScenarioError::InvalidSubstitution(
            SubstitutionError::BroadRegion,
        ));
    }
    Ok(())
}

fn validate_placement(placement: &TextPlacement, rectangle: CellRect) -> Result<(), ScenarioError> {
    let wrapping_matches = match placement.wrapping {
        Wrapping::NoWrap => rectangle.rows == 1 && !placement.text.contains('\n'),
        Wrapping::HardWrap => placement.text.split('\n').count() == usize::from(rectangle.rows),
    };
    if !wrapping_matches {
        return Err(ScenarioError::InvalidSubstitution(
            SubstitutionError::WrappingMismatch,
        ));
    }
    let width = u32::from(placement.cell_width)
        + u32::from(placement.padding_left)
        + u32::from(placement.padding_right);
    if placement.cell_width == 0 || width != u32::from(rectangle.cols) {
        return Err(ScenarioError::InvalidSubstitution(
            SubstitutionError::PaddingMismatch,
        ));
    }
    Ok(())
}

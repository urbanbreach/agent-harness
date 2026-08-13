use super::*;

pub(in crate::ui) fn resolve_block_surface(
    spec: &TranscriptBlockSpec,
    mut surface: TranscriptRenderSurface,
) -> Result<TranscriptRenderSurface, TranscriptGrammarError> {
    validate_block_spec(spec)?;
    if surface
        .interaction_rows
        .as_ref()
        .is_some_and(|rows| rows.len() != surface.lines.len())
        || surface
            .selection_rows
            .as_ref()
            .is_some_and(|rows| rows.len() > surface.lines.len())
    {
        return Err(TranscriptGrammarError::RowMismatch);
    }
    surface.leading_gap_rows = spec.spacing.leading_gap_rows;
    surface.placement = spec.placement;
    Ok(surface)
}

pub(in crate::ui) fn resolve_compatibility_surfaces(
    specs: &[TranscriptBlockSpec],
    surfaces: Vec<TranscriptRenderSurface>,
) -> Result<Vec<TranscriptRenderSurface>, TranscriptGrammarError> {
    specs.iter().try_for_each(validate_block_spec)?;
    let mut previous_role = None;
    let mut cursor = 0;
    surfaces
        .into_iter()
        .map(|surface| {
            let role = role_for_surface(surface.kind);
            let indexed = specs
                .iter()
                .enumerate()
                .skip(cursor)
                .find(|(_, spec)| spec.role == role)
                .ok_or(TranscriptGrammarError::InvalidPlacement)?;
            cursor = indexed.0.saturating_add(1);
            let mut spec = indexed.1.clone();
            spec.spacing.leading_gap_rows = grammar_leading_gap(previous_role, role);
            if let TranscriptBlockContent::Footer { lifecycle, .. } = spec.content {
                spec.placement = footer_placement(lifecycle);
            }
            previous_role = Some(role);
            resolve_block_surface(&spec, surface)
        })
        .collect()
}

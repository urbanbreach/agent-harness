use super::*;

pub(in crate::ui) fn resolve_block_surface(
    spec: &TranscriptBlockSpec,
    surface: TranscriptVisualEntryDraft,
) -> Result<ResolvedTranscriptVisualEntryDraft, TranscriptGrammarError> {
    resolve_block_surface_for_activity(0, spec, surface)
}

fn resolve_block_surface_for_activity(
    activity_first_seq: u64,
    spec: &TranscriptBlockSpec,
    mut surface: TranscriptVisualEntryDraft,
) -> Result<ResolvedTranscriptVisualEntryDraft, TranscriptGrammarError> {
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
    surface.trailing_gap_rows = spec.spacing.trailing_gap_rows;
    surface.placement = spec.placement;
    surface.show_outer_rail |= block_has_visible_accent(spec);
    surface.selected_rail |= block_is_selected(spec);
    let metadata = TranscriptVisualEntryMetadata::from_spec(activity_first_seq, spec, &surface);
    Ok(ResolvedTranscriptVisualEntryDraft {
        metadata,
        draft: surface,
    })
}

fn block_has_visible_accent(spec: &TranscriptBlockSpec) -> bool {
    match &spec.content {
        TranscriptBlockContent::UserMessage { state, .. } => {
            !matches!(state, TranscriptPromptState::Idle)
        }
        TranscriptBlockContent::AssistantBody { streaming, .. } => *streaming,
        TranscriptBlockContent::Reasoning { active, .. } => *active,
        TranscriptBlockContent::Tool { policy, .. } => {
            matches!(
                policy.status,
                TranscriptToolStatus::Running | TranscriptToolStatus::Waiting
            ) || spec.motion != TranscriptBlockMotionDemand::None
        }
        TranscriptBlockContent::Footer { .. } => false,
        TranscriptBlockContent::Error { .. } | TranscriptBlockContent::Compaction { .. } => false,
        #[cfg(test)]
        TranscriptBlockContent::Synthetic { .. } => spec.chrome.accent,
    }
}

fn block_is_selected(spec: &TranscriptBlockSpec) -> bool {
    spec.interaction.selected
        || matches!(
            spec.content,
            TranscriptBlockContent::UserMessage {
                state: TranscriptPromptState::Selected,
                ..
            }
        )
}

pub(in crate::ui) fn resolve_entry_surfaces(
    activity_first_seq: u64,
    specs: &[TranscriptBlockSpec],
    surfaces: Vec<TranscriptVisualEntryDraft>,
) -> Result<Vec<ResolvedTranscriptVisualEntryDraft>, TranscriptGrammarError> {
    if specs.len() != surfaces.len() {
        return Err(TranscriptGrammarError::RowMismatch);
    }
    specs.iter().try_for_each(validate_block_spec)?;
    let mut previous_spec = None;
    specs
        .iter()
        .zip(surfaces)
        .enumerate()
        .map(|(index, (source_spec, surface))| {
            let mut spec = source_spec.clone();
            spec.spacing.leading_gap_rows = grammar_leading_gap(previous_spec, source_spec);
            spec.spacing.trailing_gap_rows = usize::from(index + 1 == specs.len());
            if let TranscriptBlockContent::Footer { lifecycle, .. } = &spec.content {
                spec.placement = footer_placement(*lifecycle);
            }
            previous_spec = Some(source_spec);
            resolve_block_surface_for_activity(activity_first_seq, &spec, surface)
        })
        .collect()
}

#[cfg(test)]
pub(in crate::ui) fn resolve_compatibility_surfaces(
    specs: &[TranscriptBlockSpec],
    surfaces: Vec<TranscriptVisualEntryDraft>,
) -> Result<Vec<TranscriptVisualEntryDraft>, TranscriptGrammarError> {
    specs.iter().try_for_each(validate_block_spec)?;
    let selected_specs = surfaces
        .iter()
        .scan(0, |cursor, surface| {
            let role = role_for_surface(surface.kind);
            let indexed = specs
                .iter()
                .enumerate()
                .skip(*cursor)
                .find(|(_, spec)| spec.role == role)?;
            *cursor = indexed.0.saturating_add(1);
            Some(indexed.1.clone())
        })
        .collect::<Vec<_>>();
    resolve_entry_surfaces(0, &selected_specs, surfaces).map(|entries| {
        entries
            .into_iter()
            .map(|entry| entry.draft)
            .collect::<Vec<_>>()
    })
}

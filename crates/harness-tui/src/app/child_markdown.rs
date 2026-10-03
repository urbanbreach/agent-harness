use super::*;
use crate::transcript_block_viewer::ViewerMode;

impl AppState {
    pub(super) fn toggle_child_markdown(&mut self) {
        let Some(entry) = self.selected_transcript_entry() else {
            return;
        };
        if !matches!(
            entry.kind,
            ui::TranscriptRenderSurfaceKind::AssistantBody
                | ui::TranscriptRenderSurfaceKind::AssistantReasoning
        ) {
            return;
        }
        let previous = self
            .transcript_view
            .markdown_modes
            .iter_mut()
            .find(|(id, _)| *id == entry.id);
        if let Some((_, mode)) = previous {
            *mode = if *mode == ViewerMode::Raw {
                ViewerMode::Wrapped
            } else {
                ViewerMode::Raw
            };
        } else {
            self.transcript_view
                .markdown_modes
                .push((entry.id, ViewerMode::Raw));
        }
        if let Some(viewer) = &mut self.transcript_viewer {
            let _ = viewer.toggle_mode();
        }
        self.bump_transcript_render_epoch();
    }
}

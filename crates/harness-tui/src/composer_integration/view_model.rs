use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::attachment_lifecycle::{MimeKind, Preview};
use crate::completion_controller::{CompletionDropdownGeometry, CompletionItem, CompletionStatus};
use crate::composer_atoms::{AtomId, AttachmentId, WrappedLine};
use crate::ghost_suggestions::muted_style;
use crate::prompt_queue_actions::{QueueLifecycle, QueueVisuals};
use crate::shell_geometry::{cursor_for, layout_for, CursorPlacement, FocusTarget, ShellState};
use crate::theme_tokens::{BorderRole, ViewportId};

use super::slice::ComposerSlice;
use super::view_helpers::{atom_char_count, preview_label};
use super::ComposerEditorModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerBorderViewModel {
    pub rect: Rect,
    pub role: BorderRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionViewModel {
    pub geometry: CompletionDropdownGeometry,
    pub status: CompletionStatus,
    pub items: Vec<CompletionItem>,
    pub selected: Option<usize>,
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhostSuggestionViewModel {
    pub text: String,
    pub style: Style,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentPreviewViewModel {
    pub id: AtomId,
    pub attachment_id: AttachmentId,
    pub rect: Rect,
    pub mime: MimeKind,
    pub preview: Preview,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerViewModel {
    pub editor: ComposerEditorModel,
    pub viewport_id: ViewportId,
    pub viewport: Rect,
    pub composer: Rect,
    pub input: Rect,
    pub border: ComposerBorderViewModel,
    pub lifecycle: QueueLifecycle,
    pub shell_state: ShellState,
    pub focus: FocusTarget,
    pub cursor: CursorPlacement,
    pub text: String,
    pub wrapped_lines: Vec<WrappedLine>,
    pub attachments: Vec<AttachmentPreviewViewModel>,
    pub completion: Option<CompletionViewModel>,
    pub ghost: Option<GhostSuggestionViewModel>,
    pub queue_badges: Vec<String>,
    pub queue_visuals: QueueVisuals,
}

impl ComposerViewModel {
    pub fn wrapped_atom_ids(&self) -> Vec<AtomId> {
        self.wrapped_lines
            .iter()
            .flat_map(|line| line.atom_ids.iter().copied())
            .collect()
    }
}

pub(super) fn build(slice: &ComposerSlice, viewport_id: ViewportId) -> ComposerViewModel {
    let (width, height) = viewport_id.dimensions();
    let viewport = Rect::new(0, 0, width, height);
    build_for_rect(slice, viewport_id, viewport)
}

pub(super) fn build_for_rect(
    slice: &ComposerSlice,
    viewport_id: ViewportId,
    viewport: Rect,
) -> ComposerViewModel {
    let shell_state = slice.shell_state();
    let regions = layout_for(viewport_id, shell_state);
    let text = slice.editor.text();
    let wrap_width = regions.composer.width.saturating_sub(2).max(1);
    let editor = ComposerEditorModel::for_layout(
        slice.editor(),
        wrap_width,
        usize::from(regions.composer.height.max(1)),
    );
    let wrapped_lines = slice.editor.buffer().wrap(wrap_width);
    let cursor_chars = slice
        .editor
        .buffer()
        .atoms()
        .iter()
        .take(slice.editor.cursor().insertion_index())
        .map(atom_char_count)
        .sum();
    let mut cursor = cursor_for(&regions, shell_state, &text, cursor_chars);
    let focus = focus_target(slice);
    cursor.visible = cursor.visible && focus == FocusTarget::Prompt;
    let input = input_rect(regions.composer);
    let completion = build_completion(slice, viewport, shell_state);
    let attachments = slice
        .attachments
        .iter()
        .enumerate()
        .map(|(index, entry)| attachment_preview(entry, regions.composer, index))
        .collect();
    let ghost = slice
        .suggestions
        .ghost_for(&text)
        .map(|remainder| GhostSuggestionViewModel {
            text: remainder.to_owned(),
            style: muted_style(),
        });
    ComposerViewModel {
        editor,
        viewport_id,
        viewport,
        composer: regions.composer,
        input,
        border: ComposerBorderViewModel {
            rect: regions.composer,
            role: border_role(slice, shell_state),
        },
        lifecycle: slice.queue.lifecycle,
        shell_state,
        focus,
        cursor,
        text,
        wrapped_lines,
        attachments,
        completion,
        ghost,
        queue_badges: slice
            .queue
            .queued
            .iter()
            .map(|item| item.id.clone())
            .collect(),
        queue_visuals: slice.queue.visuals(),
    }
}

fn build_completion(
    slice: &ComposerSlice,
    viewport: Rect,
    shell_state: ShellState,
) -> Option<CompletionViewModel> {
    let trigger = slice.completion_trigger.as_ref()?;
    let status = slice.completion.status();
    if status == CompletionStatus::Hidden {
        return None;
    }
    let geometry = crate::completion_controller::ShellCompletionGeometry::calculate(
        &crate::completion_controller::CompletionGeometryInput {
            viewport,
            state: shell_state,
            buffer: slice.editor.buffer(),
            cursor: slice.editor.cursor(),
            item_count: slice.completion_items.len(),
            max_rows: 5,
        },
    );
    Some(CompletionViewModel {
        geometry,
        status,
        items: slice.completion_items.clone(),
        selected: slice.completion.selected_index(),
        query: trigger.query.clone(),
    })
}

fn attachment_preview(
    entry: &super::slice::AttachmentEntry,
    composer: Rect,
    index: usize,
) -> AttachmentPreviewViewModel {
    let available = composer.width.saturating_sub(2).max(1);
    let x = composer
        .x
        .saturating_add(1)
        .saturating_add(u16::try_from(index.saturating_mul(8)).unwrap_or(u16::MAX));
    let width = available
        .saturating_sub(x.saturating_sub(composer.x))
        .clamp(1, 12);
    AttachmentPreviewViewModel {
        id: entry.atom_id(),
        attachment_id: entry.id,
        rect: Rect::new(
            x.min(composer.right().saturating_sub(width)),
            composer.y,
            width,
            1,
        ),
        mime: entry.attachment.mime(),
        preview: entry.attachment.preview().clone(),
        label: preview_label(entry.attachment.preview()),
    }
}

fn input_rect(composer: Rect) -> Rect {
    Rect::new(
        composer.x.saturating_add(1),
        composer.y.saturating_add(1.min(composer.height)),
        composer.width.saturating_sub(2),
        composer.height.saturating_sub(2).max(1),
    )
}

fn border_role(slice: &ComposerSlice, shell_state: ShellState) -> BorderRole {
    if slice.is_prompt_focused() && shell_state.is_editable() {
        BorderRole::Focus
    } else if shell_state == ShellState::Failed {
        BorderRole::Strong
    } else {
        BorderRole::Subtle
    }
}

fn focus_target(slice: &ComposerSlice) -> FocusTarget {
    match slice.interaction_state().focus {
        crate::app::Focus::List => FocusTarget::Shell,
        crate::app::Focus::Details => FocusTarget::Scrollback,
        crate::app::Focus::Terminal => FocusTarget::Shell,
        crate::app::Focus::Prompt => FocusTarget::Prompt,
    }
}

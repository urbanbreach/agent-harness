use std::sync::Arc;

use ratatui::style::Color;

use super::super::ui_transcript_layout::{measure_transcript_layout, MeasuredTranscriptLayout};
use super::{build_transcript_render_surfaces, prepare_transcript_sections, TranscriptTurnSection};
use crate::{app::AppState, theme::Theme};

#[derive(Debug, Default)]
pub(crate) struct PreparedTranscript {
    epoch: u64,
    layouts: Vec<PreparedLayout>,
    dirty_from: Option<usize>,
}

#[derive(Debug)]
struct PreparedLayout {
    key: u64,
    settings: u64,
    theme: Theme,
    width: u16,
    surface: Color,
    sections: Arc<[Arc<TranscriptTurnSection>]>,
    layout: MeasuredTranscriptLayout,
}

impl PreparedTranscript {
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    pub(crate) fn invalidate_from(&mut self, index: usize) {
        if index == 0 {
            self.layouts.clear();
        }
        self.epoch = self.epoch.wrapping_add(1);
        self.dirty_from = Some(self.dirty_from.map_or(index, |old| old.min(index)));
    }

    fn find(&self, key: u64, theme: &Theme, width: u16, surface: Color) -> Option<&PreparedLayout> {
        self.layouts.iter().find(|entry| {
            entry.key == key
                && entry.theme == *theme
                && entry.width == width
                && entry.surface == surface
        })
    }

    fn build(&self, app: &AppState, theme: &Theme, width: u16, surface: Color) -> PreparedLayout {
        let key = app.transcript_render_cache_key();
        let settings = app.transcript_settings_key();
        let current =
            self.layouts.iter().rev().find(|entry| {
                entry.key == key && entry.theme == *theme && entry.surface == surface
            });
        let sections = current.map_or_else(
            || {
                let previous = self.layouts.last().filter(|entry| {
                    entry.settings == settings && entry.theme == *theme && entry.surface == surface
                });
                let dirty_from = previous.map_or(0, |_| self.dirty_from.unwrap_or(usize::MAX));
                Arc::from(prepare_transcript_sections(
                    app,
                    previous.map_or(&[], |entry| &entry.sections),
                    dirty_from,
                ))
            },
            |entry| Arc::clone(&entry.sections),
        );
        let previous = self.layouts.iter().rev().find(|entry| {
            entry.theme == *theme && entry.width == width && entry.surface == surface
        });
        let layout = measure_transcript_layout(
            &sections,
            theme,
            width,
            surface,
            |section| section.activity_first_seq,
            |index, section| {
                let previous = previous?;
                Arc::ptr_eq(previous.sections.get(index)?, section)
                    .then(|| previous.layout.sections.get(index).cloned())
                    .flatten()
            },
            |section, theme, width, surface| {
                build_transcript_render_surfaces(section, theme, width, surface)
            },
        );
        PreparedLayout {
            key,
            settings,
            theme: *theme,
            width,
            surface,
            sections,
            layout,
        }
    }
}

pub(in crate::ui) fn prepare_width(app: &mut AppState, theme: &Theme, width: u16, surface: Color) {
    let prepared = &app.transcript_view.prepared;
    if prepared
        .find(app.transcript_render_cache_key(), theme, width, surface)
        .is_some()
    {
        return;
    }
    let entry = prepared.build(app, theme, width, surface);
    let prepared = &mut app.transcript_view.prepared;
    prepared
        .layouts
        .retain(|old| old.theme == *theme && (old.width != width || old.surface != surface));
    prepared.layouts.push(entry);
    // Two widths (with and without scrollbar) at the current and previous size.
    if prepared.layouts.len() > 4 {
        prepared.layouts.remove(0);
    }
    prepared.dirty_from = None;
}

pub(super) fn with_layout<R>(
    app: &AppState,
    theme: &Theme,
    width: u16,
    surface: Color,
    read: impl FnOnce(&MeasuredTranscriptLayout) -> R,
) -> R {
    let prepared = &app.transcript_view.prepared;
    if let Some(entry) = prepared.find(app.transcript_render_cache_key(), theme, width, surface) {
        return read(&entry.layout);
    }
    // Public projection also supports callers that have not prepared a frame.
    // Its temporary layout is local; immutable reads never populate a cache.
    read(&prepared.build(app, theme, width, surface).layout)
}

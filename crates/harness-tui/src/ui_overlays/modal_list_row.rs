use super::*;

const DEFAULT_INSET: u16 = 2;
const COMPACT_INSET: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalListRowState {
    pub(crate) selected: bool,
    pub(crate) hovered: bool,
    pub(crate) dimmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalListRowLayout {
    pub(crate) content: Rect,
    pub(crate) scrollbar: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalScrollbarGeometry {
    pub(crate) track: Rect,
    pub(crate) thumb: Rect,
}

impl ModalScrollbarGeometry {
    pub(crate) const fn contains(self, column: u16, row: u16) -> bool {
        column >= self.track.x
            && column < self.track.right()
            && row >= self.track.y
            && row < self.track.bottom()
    }

    pub(crate) fn drag_offset(
        self,
        anchor_row: u16,
        anchor_offset: usize,
        row: u16,
        max_scroll: usize,
    ) -> usize {
        let travel = usize::from(self.track.height.saturating_sub(self.thumb.height));
        if travel == 0 || max_scroll == 0 {
            return 0;
        }
        let distance = usize::from(row.abs_diff(anchor_row));
        let offset_delta = distance.saturating_mul(max_scroll) / travel;
        if row >= anchor_row {
            anchor_offset.saturating_add(offset_delta).min(max_scroll)
        } else {
            anchor_offset.saturating_sub(offset_delta)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalListRowPresentation {
    pub(crate) layout: ModalListRowLayout,
    pub(crate) style: Style,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalListRowSpec {
    pub(crate) area: Rect,
    pub(crate) state: ModalListRowState,
    pub(crate) max_scroll: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalListScrollbarSpec {
    pub(crate) area: Rect,
    pub(crate) offset: usize,
    pub(crate) max_scroll: usize,
}

pub(crate) const fn modal_list_row_layout(area: Rect, max_scroll: usize) -> ModalListRowLayout {
    let scrollbar_width = if max_scroll > 0 { 1 } else { 0 };
    let available = area.width.saturating_sub(scrollbar_width);
    let inset = if available >= 5 {
        DEFAULT_INSET
    } else if available >= 3 {
        COMPACT_INSET
    } else {
        0
    };
    let content = Rect::new(
        area.x.saturating_add(inset),
        area.y,
        available.saturating_sub(inset.saturating_mul(2)),
        area.height,
    );
    let scrollbar = if max_scroll > 0 {
        Some(Rect::new(
            area.right().saturating_sub(inset).saturating_sub(1),
            area.y,
            1,
            area.height,
        ))
    } else {
        None
    };
    ModalListRowLayout { content, scrollbar }
}

pub(crate) fn modal_list_row(theme: &Theme, spec: ModalListRowSpec) -> ModalListRowPresentation {
    let layout = modal_list_row_layout(spec.area, spec.max_scroll);
    let background = if spec.state.hovered {
        theme.surface.hover
    } else if spec.state.selected {
        theme.question_prompt.selected
    } else {
        ui_chrome::command_palette_surface(theme)
    };
    let foreground = if spec.state.dimmed && !spec.state.selected {
        theme.text.tertiary
    } else {
        theme.text.primary
    };
    let style =
        Style::default()
            .fg(foreground)
            .bg(background)
            .add_modifier(if spec.state.selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
    ModalListRowPresentation { layout, style }
}

pub(crate) fn modal_scrollbar_geometry(
    area: Rect,
    offset: usize,
    max_scroll: usize,
) -> Option<ModalScrollbarGeometry> {
    let track = modal_list_row_layout(area, max_scroll).scrollbar?;
    let visible = usize::from(track.height);
    let total = visible.saturating_add(max_scroll);
    let thumb_height = visible
        .saturating_mul(visible)
        .saturating_add(total.saturating_sub(1))
        .checked_div(total)
        .unwrap_or(1)
        .clamp(1, visible);
    let travel = visible.saturating_sub(thumb_height);
    let thumb_offset = offset
        .min(max_scroll)
        .saturating_mul(travel)
        .checked_div(max_scroll)
        .unwrap_or(0);
    Some(ModalScrollbarGeometry {
        track,
        thumb: Rect::new(
            track.x,
            track
                .y
                .saturating_add(u16::try_from(thumb_offset).unwrap_or(u16::MAX)),
            track.width,
            u16::try_from(thumb_height).unwrap_or(track.height),
        ),
    })
}

pub(crate) const fn modal_list_row_text_style(row_style: Style, foreground: Color) -> Style {
    row_style.fg(foreground)
}

pub(crate) fn render_modal_list_scrollbar(
    frame: &mut Frame,
    theme: &Theme,
    spec: ModalListScrollbarSpec,
) {
    let Some(scrollbar) = modal_scrollbar_geometry(spec.area, spec.offset, spec.max_scroll) else {
        return;
    };
    for row in 0..scrollbar.track.height {
        let row_y = scrollbar.track.y.saturating_add(row);
        let is_thumb = row_y >= scrollbar.thumb.y && row_y < scrollbar.thumb.bottom();
        frame.render_widget(
            Paragraph::new(if is_thumb { "█" } else { " " }).style(
                Style::default()
                    .fg(if is_thumb {
                        theme.text.tertiary
                    } else {
                        theme.scrollbar.track
                    })
                    .bg(ui_chrome::command_palette_surface(theme)),
            ),
            Rect::new(scrollbar.track.x, row_y, 1, 1),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presentation(theme: &Theme, state: ModalListRowState) -> ModalListRowPresentation {
        modal_list_row(
            theme,
            ModalListRowSpec {
                area: Rect::new(0, 0, 40, 1),
                state,
                max_scroll: 0,
            },
        )
    }

    #[test]
    fn modal_row_state_matrix_matches_reference_precedence() {
        // arrange
        // Given: every standalone modal-row state plus selected-and-dimmed.
        let theme = Theme::harness_chat();
        let cases = [
            (
                ModalListRowState {
                    selected: false,
                    hovered: false,
                    dimmed: false,
                },
                ui_chrome::command_palette_surface(&theme),
                theme.text.primary,
                false,
            ),
            (
                ModalListRowState {
                    selected: false,
                    hovered: true,
                    dimmed: false,
                },
                theme.surface.hover,
                theme.text.primary,
                false,
            ),
            (
                ModalListRowState {
                    selected: true,
                    hovered: false,
                    dimmed: false,
                },
                theme.question_prompt.selected,
                theme.text.primary,
                true,
            ),
            (
                ModalListRowState {
                    selected: false,
                    hovered: false,
                    dimmed: true,
                },
                ui_chrome::command_palette_surface(&theme),
                theme.text.tertiary,
                false,
            ),
            (
                ModalListRowState {
                    selected: true,
                    hovered: false,
                    dimmed: true,
                },
                theme.question_prompt.selected,
                theme.text.primary,
                true,
            ),
        ];

        // act
        // When/Then: each state resolves independently to the semantic band and text role.
        for (state, background, foreground, bold) in cases {
            let row = presentation(&theme, state);
            // assert
            assert_eq!(
                (
                    row.style.bg,
                    row.style.fg,
                    row.style.add_modifier.contains(Modifier::BOLD)
                ),
                (Some(background), Some(foreground), bold),
                "state={state:?}"
            );
        }
    }

    #[test]
    fn modal_row_viewport_gutters_keep_band_scrollbar_and_border_disjoint() {
        // arrange
        // Given: list rows inside the compact and wide modal border viewports.
        let cases = [Rect::new(1, 3, 58, 12), Rect::new(1, 3, 118, 32)];

        // act
        // When/Then: the inset band remains inside borders and left of the scrollbar lane.
        for area in cases {
            let layout = modal_list_row_layout(area, 20);
            let scrollbar = layout.scrollbar.expect("scrollbar lane");
            // assert
            assert!(layout.content.x > area.x, "area={area:?} layout={layout:?}");
            assert!(
                layout.content.right() <= scrollbar.x,
                "area={area:?} layout={layout:?}"
            );
            assert!(
                scrollbar.right() < area.right(),
                "area={area:?} layout={layout:?}"
            );
        }
    }

    #[test]
    fn selected_hovered_row_uses_hover_band_and_selected_text() {
        // arrange
        // Given: a row that is both keyboard-selected and pointer-hovered.
        let theme = Theme::harness_chat();
        let spec = ModalListRowSpec {
            area: Rect::new(0, 0, 40, 1),
            state: ModalListRowState {
                selected: true,
                hovered: true,
                dimmed: false,
            },
            max_scroll: 0,
        };

        // When: the shared modal row presentation is resolved.
        let row = modal_list_row(&theme, spec);

        // act
        // Then: Grok's hover material wins while selected text remains bold.
        // assert
        assert_eq!(row.style.bg, Some(theme.surface.hover));
        assert_eq!(row.style.fg, Some(theme.text.primary));
        assert!(row.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn selected_row_text_override_preserves_band_and_bold_modifier() {
        // arrange
        // Given: a selected and hovered row whose metadata needs a quieter foreground.
        let theme = Theme::harness_chat();
        let row = modal_list_row(
            &theme,
            ModalListRowSpec {
                area: Rect::new(0, 0, 40, 1),
                state: ModalListRowState {
                    selected: true,
                    hovered: true,
                    dimmed: false,
                },
                max_scroll: 0,
            },
        );

        // When: a caller derives a text span with a different semantic foreground.
        let text_style = modal_list_row_text_style(row.style, theme.text.tertiary);

        // act
        // Then: only the foreground changes; the hover band and selected weight remain.
        // assert
        assert_eq!(text_style.fg, Some(theme.text.tertiary));
        assert_eq!(text_style.bg, Some(theme.surface.hover));
        assert!(text_style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn modal_row_layout_reserves_insets_and_scrollbar() {
        // arrange
        // Given: a wide row whose list can scroll.
        let area = Rect::new(10, 4, 40, 1);

        // When: the shared geometry is measured.
        let layout = modal_list_row_layout(area, 8);

        // act
        // Then: two-cell gutters remain outside the band and scrollbar lane.
        // assert
        assert_eq!(layout.content, Rect::new(12, 4, 35, 1));
        assert_eq!(layout.scrollbar, Some(Rect::new(47, 4, 1, 1)));
    }

    #[test]
    fn modal_row_layout_uses_compact_inset_when_width_is_tight() {
        // arrange
        // Given: a four-cell row without scrolling.
        let area = Rect::new(3, 2, 4, 1);

        // When: the shared geometry is measured.
        let layout = modal_list_row_layout(area, 0);

        // act
        // Then: one-cell gutters preserve a two-cell content band.
        // assert
        assert_eq!(layout.content, Rect::new(4, 2, 2, 1));
        assert_eq!(layout.scrollbar, None);
    }

    #[test]
    fn scrollbar_drag_uses_press_anchor_and_clamps_to_range() {
        // arrange
        // Given: a proportional thumb at the start of a ten-row scroll range.
        let scrollbar =
            modal_scrollbar_geometry(Rect::new(10, 5, 40, 5), 0, 10).expect("scrollbar geometry");

        // When: the pointer drags beyond the bottom of the track.
        let offset = scrollbar.drag_offset(scrollbar.thumb.y, 0, u16::MAX, 10);

        // act
        // Then: the pointer anchor maps to the clamped end of the range.
        // assert
        assert_eq!(offset, 10);
    }
}

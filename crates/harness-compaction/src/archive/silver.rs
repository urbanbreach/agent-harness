//! Bundled Silver TrueType font: exact coverage queries and glyph rasterization.
//! Silver is a 16 px design with a 16 by 16 cell; it licenses under CC BY 4.0.
use fontdue::{Font, FontSettings, Metrics};
use std::{collections::HashMap, sync::LazyLock};

/// Design pixel size and natural cell edge of Silver.
pub(super) const DESIGN: u32 = 16;
pub(super) const DESIGN_PX: f32 = 16.0;
/// Stored design ascent at the design size (font units 1200 of 1900 per em), used only
/// if the font has no horizontal line metrics.
const DESIGN_ASCENT: f32 = DESIGN_PX * 1200.0 / 1900.0;

fn font() -> Option<&'static Font> {
    static SILVER: LazyLock<Option<Font>> = LazyLock::new(|| {
        Font::from_bytes(
            &include_bytes!("../../fonts/Silver.ttf")[..],
            FontSettings::default(),
        )
        .ok()
    });
    SILVER.as_ref()
}

/// Coverage is membership in the embedded character map: a nonzero glyph index.
pub(super) fn covers(ch: char) -> bool {
    font().is_some_and(|font| font.lookup_glyph_index(ch) != 0)
}

/// Baseline distance from the top of a line box at `px`.
pub(super) fn ascent(px: f32) -> f32 {
    font()
        .and_then(|font| font.horizontal_line_metrics(px))
        .map_or(DESIGN_ASCENT * px / DESIGN_PX, |line| line.ascent)
}

/// Coverage bitmap (row-major, one byte per pixel) of a glyph Silver contains.
pub(super) fn rasterize(ch: char, px: f32) -> Option<(Metrics, Vec<u8>)> {
    let font = font()?;
    (font.lookup_glyph_index(ch) != 0).then(|| font.rasterize(ch, px))
}

/// Per-frame glyph cache: sources repeat characters heavily.
#[derive(Default)]
pub(super) struct Glyphs(HashMap<(char, u32), Option<(Metrics, Vec<u8>)>>);

impl Glyphs {
    pub(super) fn get(&mut self, ch: char, px: f32) -> Option<&(Metrics, Vec<u8>)> {
        self.0
            .entry((ch, px.to_bits()))
            .or_insert_with(|| rasterize(ch, px))
            .as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_is_cmap_membership() {
        for (ch, expected) in [
            ('A', true),
            ('你', true),
            ('か', true),
            ('한', true),
            ('\u{e000}', false),
            ('\u{10ffff}', false),
        ] {
            assert_eq!(covers(ch), expected, "{ch:?}");
        }
    }

    #[test]
    fn design_size_glyphs_fit_the_cell() {
        let Some((metrics, coverage)) = rasterize('你', 16.0) else {
            panic!("Silver covers CJK ideographs");
        };
        assert_eq!(coverage.len(), metrics.width * metrics.height);
        assert!(coverage.iter().any(|&c| c > 0));
        assert!(metrics.advance_width.ceil() <= 16.0);
    }
}

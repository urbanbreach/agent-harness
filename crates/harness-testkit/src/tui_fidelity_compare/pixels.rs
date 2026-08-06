use image::{ImageFormat, RgbaImage};

use super::error::ComparatorError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x.saturating_add(self.width)
            && y < self.y.saturating_add(self.height)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityPixelSpan {
    pub name: String,
    pub rectangle: PixelRect,
}

impl IdentityPixelSpan {
    pub fn new(name: impl Into<String>, rectangle: PixelRect) -> Self {
        Self {
            name: name.into(),
            rectangle,
        }
    }

    pub fn from_cell_rect(
        name: impl Into<String>,
        rectangle: crate::tui_fidelity::CellRect,
        cell_width: u32,
        cell_height: u32,
    ) -> Self {
        Self::new(
            name,
            PixelRect::new(
                u32::from(rectangle.col).saturating_mul(cell_width),
                u32::from(rectangle.row).saturating_mul(cell_height),
                u32::from(rectangle.cols).saturating_mul(cell_width),
                u32::from(rectangle.rows).saturating_mul(cell_height),
            ),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelDiffRecord {
    pub x: u32,
    pub y: u32,
    pub expected: [u8; 4],
    pub observed: [u8; 4],
}

pub fn compare_png_bytes(
    expected: &[u8],
    actual: &[u8],
    identity_spans: &[IdentityPixelSpan],
) -> Result<(), ComparatorError> {
    let expected = decode_png("reference", expected)?;
    let actual = decode_png("candidate", actual)?;
    if expected.dimensions() != actual.dimensions() {
        return Err(ComparatorError::Invalid {
            detail: format!(
                "PNG dimensions differ: reference {:?}, candidate {:?}",
                expected.dimensions(),
                actual.dimensions()
            ),
        });
    }
    validate_spans(identity_spans, expected.width(), expected.height())?;
    let mut diffs = Vec::new();
    for y in 0..expected.height() {
        for x in 0..expected.width() {
            let left = expected.get_pixel(x, y).0;
            let right = actual.get_pixel(x, y).0;
            if left != right
                && !identity_spans
                    .iter()
                    .any(|span| span.rectangle.contains(x, y))
            {
                diffs.push(PixelDiffRecord {
                    x,
                    y,
                    expected: left,
                    observed: right,
                });
            }
        }
    }
    if diffs.is_empty() {
        Ok(())
    } else {
        let diffs_len = diffs.len();
        Err(ComparatorError::Pixels { diffs, diffs_len })
    }
}

pub fn compare_png(
    expected: &[u8],
    actual: &[u8],
    identity_spans: &[IdentityPixelSpan],
) -> Result<(), ComparatorError> {
    compare_png_bytes(expected, actual, identity_spans)
}

fn decode_png(side: &str, bytes: &[u8]) -> Result<RgbaImage, ComparatorError> {
    Ok(image::load_from_memory_with_format(bytes, ImageFormat::Png)
        .map_err(|error| ComparatorError::PngDecode {
            side: side.to_owned(),
            detail: error.to_string(),
        })?
        .into_rgba8())
}

fn validate_spans(
    spans: &[IdentityPixelSpan],
    width: u32,
    height: u32,
) -> Result<(), ComparatorError> {
    for span in spans {
        let right = span.rectangle.x.saturating_add(span.rectangle.width);
        let bottom = span.rectangle.y.saturating_add(span.rectangle.height);
        if right > width || bottom > height {
            return Err(ComparatorError::Invalid {
                detail: format!("identity span `{}` exceeds PNG bounds", span.name),
            });
        }
    }
    Ok(())
}

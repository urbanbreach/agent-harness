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
        image_width: u32,
        image_height: u32,
        viewport_cols: u16,
        viewport_rows: u16,
    ) -> Self {
        let x0 = proportional_boundary(rectangle.col, image_width, viewport_cols);
        let x1 = proportional_boundary(
            rectangle.col.saturating_add(rectangle.cols),
            image_width,
            viewport_cols,
        );
        let y0 = proportional_boundary(rectangle.row, image_height, viewport_rows);
        let y1 = proportional_boundary(
            rectangle.row.saturating_add(rectangle.rows),
            image_height,
            viewport_rows,
        );
        Self::new(
            name,
            PixelRect::new(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0)),
        )
    }
}

const fn proportional_boundary(cell: u16, pixels: u32, cells: u16) -> u32 {
    let cells = cells as u32;
    let cell = cell as u32;
    pixels / cells * cell + pixels % cells * cell / cells
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

#[cfg(test)]
mod tests {
    use super::{IdentityPixelSpan, PixelRect};
    use crate::tui_fidelity::CellRect;

    #[test]
    fn typed_identity_rect_uses_proportional_boundaries_without_changing_divisible_projection() {
        // arrange
        let rectangle = CellRect {
            col: 44,
            row: 29,
            cols: 1,
            rows: 1,
        };

        // act
        let fractional = IdentityPixelSpan::from_cell_rect("typed", rectangle, 1806, 1081, 100, 30);
        let divisible = IdentityPixelSpan::from_cell_rect("typed", rectangle, 1800, 1080, 100, 30);

        // assert
        assert_eq!(fractional.rectangle, PixelRect::new(794, 1044, 18, 37));
        assert!(!fractional.rectangle.contains(793, 1044));
        assert!(fractional.rectangle.contains(810, 1044));
        assert!(fractional.rectangle.contains(811, 1080));
        assert!(!fractional.rectangle.contains(812, 1080));
        assert_eq!(divisible.rectangle, PixelRect::new(792, 1044, 18, 36));
    }
}

use super::canvas::{Canvas, BAND, BLACK, DIM};
use super::layout::cell_width;
use super::{fonts, silver, ArchiveError, ArchiveShape, Ink, RenderedFrame, Thumbnail};

const STOPWORDS: &str = "the a an and or of to in on at as is are was were be been by for with that this it its from had has have not but he she his her they their them which also who whom when where while will would could should there then than into over under about after before between during each such these those some most more other only same so";

fn letter(ch: char) -> bool {
    matches!(ch, 'A'..='Z' | 'a'..='z' | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{ff}')
}

fn layout(
    text: &str,
    shape: &ArchiveShape,
    cols: u32,
    rows: u32,
    mut visit: impl FnMut(char, u32, u32, u8),
) -> (u32, u64) {
    let doc = shape.columns == 2;
    let mut column = 0;
    let mut line = 0;
    let mut used = 1;
    let mut chars = 0;
    let mut dim = false;
    let mut sentence = 1_u8;
    let mut word_end = 0;
    let mut word_dim = false;
    let mut iter = text.char_indices().peekable();
    while let Some((index, ch)) = iter.next() {
        match ch {
            '\u{e}' => {
                dim = true;
                continue;
            }
            '\u{f}' => {
                dim = false;
                continue;
            }
            '\n' if doc => {
                line += 1;
                column = 0;
                continue;
            }
            _ => {}
        }
        if shape.stopword_dim && letter(ch) && index >= word_end {
            word_end = text[index..]
                .char_indices()
                .find_map(|(offset, c)| (!letter(c)).then_some(index + offset))
                .unwrap_or(text.len());
            word_dim = !dim
                && STOPWORDS
                    .split_ascii_whitespace()
                    .any(|word| word.eq_ignore_ascii_case(&text[index..word_end]));
        }
        let cells = cell_width(ch, shape.font);
        if !doc && column > 0 && column + cells > cols {
            line += 1;
            column = 0;
        }
        if line >= rows * shape.columns {
            break;
        }
        let x = if doc {
            (line / rows) * (cols + 3) + column
        } else {
            column
        };
        let y = line % rows;
        let dimmed = dim || (shape.stopword_dim && word_dim && index < word_end);
        let color = if ch == '█' {
            BLACK
        } else if dimmed {
            DIM
        } else if matches!(shape.ink, Ink::Black) {
            BLACK
        } else {
            sentence
        };
        if !doc || column < cols {
            visit(ch, x, y, color);
        }
        chars += 1;
        used = used.max(if doc && line >= rows { rows } else { y + 1 });
        if matches!(ch, '.' | '!' | '?')
            && iter
                .peek()
                .is_some_and(|(_, next)| matches!(next, ' ' | '█') || (doc && *next == '\n'))
        {
            sentence = sentence % 6 + 1;
        }
        column += cells;
        if !doc && column >= cols {
            line += 1;
            column = 0;
        }
    }
    (used, chars)
}

pub fn render_frame(text: &str, shape: &ArchiveShape) -> Result<RenderedFrame, ArchiveError> {
    let bitmap = if shape.font == "silver" {
        None
    } else {
        Some(fonts::font(shape.font).ok_or(ArchiveError::InvalidGeometry)?)
    };
    if shape.edge == 0 || shape.edge > 16384 || !matches!(shape.columns, 1 | 2) {
        return Err(ArchiveError::InvalidGeometry);
    }
    let (natural_width, natural_height) =
        bitmap.map_or((silver::DESIGN, silver::DESIGN), |f| (f.width, f.height));
    let cw = shape.cell_width.max(1);
    let ch = shape.cell_height.max(1);
    let repeat = shape.line_repeat.max(1);
    let pitch = ch
        .checked_mul(repeat)
        .ok_or(ArchiveError::InvalidGeometry)?;
    let grid_cols = shape.edge / cw;
    let rows = shape.edge / pitch;
    let cols = if shape.columns == 2 {
        grid_cols.saturating_sub(3) / 2
    } else {
        grid_cols
    };
    if cols == 0 || rows == 0 {
        return Err(ArchiveError::InvalidGeometry);
    }
    let (used, chars) = layout(text, shape, cols, rows, |_, _, _, _| {});
    let ink_height = used * pitch;
    let height = ink_height.max(64);
    let stretch = shape.stretch && (cw != natural_width || ch != natural_height);
    let raster_cw = if stretch { natural_width } else { cw };
    let raster_ch = if stretch { natural_height } else { ch };
    let width = if stretch {
        grid_cols * raster_cw
    } else {
        shape.edge
    };
    let raster_height = if stretch {
        used * repeat * raster_ch
    } else {
        height
    };
    let mut canvas = Canvas::new(width, raster_height, bitmap.is_none());
    let mut glyphs = silver::Glyphs::default();
    for row in 0..used {
        for copy in 1..repeat {
            let top = (row * repeat + copy) * raster_ch;
            canvas.fill(0, i64::from(top), width, raster_ch, BAND);
        }
    }
    layout(text, shape, cols, rows, |character, column, row, color| {
        let cell = Cell {
            width: raster_cw,
            height: raster_ch,
            span: cell_width(character, shape.font),
        };
        for copy in 0..repeat {
            let left = i64::from(column * raster_cw);
            let top = i64::from((row * repeat + copy) * raster_ch);
            draw_character(
                &mut canvas,
                &mut glyphs,
                bitmap,
                character,
                (left, top),
                cell,
                color,
            );
        }
    });
    if !stretch {
        canvas.clear_from(ink_height);
    }
    let png = if stretch {
        let canvas = image::RgbImage::from_raw(width, raster_height, canvas.into_rgb())
            .ok_or(ArchiveError::InvalidGeometry)?;
        let resized = image::imageops::resize(
            &canvas,
            grid_cols * cw,
            ink_height,
            image::imageops::FilterType::Lanczos3,
        );
        let mut output =
            image::RgbImage::from_pixel(shape.edge, height, image::Rgb([255, 255, 255]));
        image::imageops::replace(&mut output, &resized, 0, 0);
        super::canvas::encode_rgb(output.as_raw(), shape.edge, height)?
    } else {
        canvas.encode()?
    };
    Ok(RenderedFrame {
        png,
        width: shape.edge,
        height,
        chars,
    })
}

/// Requested cell size and the number of cells the character spans.
#[derive(Clone, Copy)]
struct Cell {
    width: u32,
    height: u32,
    span: u32,
}

fn draw_character(
    canvas: &mut Canvas,
    glyphs: &mut silver::Glyphs,
    bitmap: Option<&fonts::BitmapFont>,
    character: char,
    (left, top): (i64, i64),
    cell: Cell,
    color: u8,
) {
    if character == '█' {
        canvas.fill(left, top, cell.width, cell.height, BLACK);
        return;
    }
    let Some(font) = bitmap else {
        draw_silver(canvas, glyphs, character, (left, top), cell, color, None);
        return;
    };
    let Some(glyph) = font.glyphs.get(&character) else {
        draw_silver(
            canvas,
            glyphs,
            character,
            (left, top),
            cell,
            color,
            Some(font.ascent),
        );
        return;
    };
    let glyph_top = top + i64::from(font.ascent - glyph.height - glyph.y);
    for (y, bits) in glyph
        .rows
        .iter()
        .take(usize::try_from(glyph.height).unwrap_or(0))
        .enumerate()
    {
        for x in 0..glyph.width {
            if bits & (0x80 >> x) != 0 {
                canvas.set(
                    left + i64::from(glyph.x + x),
                    glyph_top + i64::try_from(y).unwrap_or(0),
                    color,
                );
            }
        }
    }
}

/// Draw a character from Silver: pure Silver frames and per-glyph bitmap fallback.
/// A character Silver lacks leaves its cells blank.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "Cell and glyph dimensions are far below f32 and i64 limits"
)]
fn draw_silver(
    canvas: &mut Canvas,
    glyphs: &mut silver::Glyphs,
    character: char,
    (left, top): (i64, i64),
    cell: Cell,
    color: u8,
    bitmap_ascent: Option<i32>,
) {
    let spanned = (cell.span * cell.width) as f32;
    let design = silver::DESIGN_PX;
    let px = design * (spanned / design).min(cell.height as f32 / design);
    let Some((metrics, coverage)) = glyphs.get(character, px) else {
        return;
    };
    let extra = i64::from(cell.span) * i64::from(cell.width) - metrics.advance_width.ceil() as i64;
    let x = left + extra.max(0) / 2 + i64::from(metrics.xmin);
    let ascent = bitmap_ascent.map_or_else(|| silver::ascent(px), |ascent| ascent as f32);
    let y = (top as f32 + ascent - metrics.height as f32 - metrics.ymin as f32).round() as i64;
    for (index, &sample) in coverage.iter().enumerate() {
        let (row, column) = (index / metrics.width, index % metrics.width);
        canvas.ink(
            x + i64::try_from(column).unwrap_or(0),
            y + i64::try_from(row).unwrap_or(0),
            color,
            sample,
        );
    }
}

/// Downsample to a packed monochrome dot grid (two by four dots per braille cell).
pub fn thumbnail(frame: &RenderedFrame, cols: u16, rows: u16) -> Thumbnail {
    let width = cols.saturating_mul(2);
    let height = rows.saturating_mul(4);
    let mut bits = vec![0; (usize::from(width) * usize::from(height)).div_ceil(8)];
    if width != 0
        && height != 0
        && let Ok(image) = image::load_from_memory_with_format(&frame.png, image::ImageFormat::Png)
    {
        let small = image::imageops::resize(
            &image.to_luma8(),
            u32::from(width),
            u32::from(height),
            image::imageops::FilterType::Triangle,
        );
        for (index, &luma) in small.as_raw().iter().enumerate() {
            if luma < 240 {
                bits[index / 8] |= 0x80 >> (index % 8);
            }
        }
    }
    Thumbnail {
        width,
        height,
        bits,
    }
}

#[cfg(test)]
mod tests {
    use super::super::canvas::PALETTE;
    use super::*;

    fn shape(id: &str, edge: u32) -> ArchiveShape {
        let mut shape = *super::super::resolve_shape("", "", Some(id));
        shape.edge = edge;
        shape
    }

    #[test]
    fn tiny_raster_has_stable_pixels_and_minimal_palette() -> Result<(), Box<dyn std::error::Error>>
    {
        let frame = render_frame("█", &shape("8x8u-bw", 16))?;
        assert_eq!((frame.width, frame.height, frame.chars), (16, 64, 1));
        let decoder = png::Decoder::new(std::io::Cursor::new(&frame.png));
        let reader = decoder.read_info()?;
        assert_eq!(
            (reader.info().color_type, reader.info().bit_depth),
            (png::ColorType::Indexed, png::BitDepth::One)
        );
        assert_eq!(
            reader.info().palette.as_deref(),
            Some(&[255, 255, 255, 0, 0, 0][..])
        );
        let rgb = image::load_from_memory(&frame.png)?.to_rgb8();
        let hash = rgb
            .as_raw()
            .iter()
            .fold(14_695_981_039_346_656_037_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(1_099_511_628_211)
            });
        assert_eq!(hash, 15_727_524_320_931_928_549);
        let thumb = thumbnail(&frame, 8, 16);
        assert_eq!((thumb.width, thumb.height), (16, 64));
        assert_eq!(
            &thumb.bits[..16],
            &[255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0]
        );
        assert!(thumb.bits[16..].iter().all(|&byte| byte == 0));
        Ok(())
    }

    #[test]
    fn repeated_and_document_rows_preserve_white_padding() -> Result<(), Box<dyn std::error::Error>>
    {
        let repeated = render_frame("x", &shape("8x8r-bw", 64))?;
        let rgb = image::load_from_memory(&repeated.png)?.to_rgb8();
        assert_eq!(rgb.get_pixel(63, 8).0, PALETTE[8]);
        assert_eq!(rgb.get_pixel(63, 16).0, PALETTE[0]);
        let doc = render_frame("█\n█\n█\n█\n█", &shape("doc-8on16-bw", 64))?;
        let rgb = image::load_from_memory(&doc.png)?.to_rgb8();
        assert_eq!((doc.height, doc.chars), (64, 5));
        assert_eq!(rgb.get_pixel(40, 0).0, PALETTE[7]);
        assert_eq!(rgb.get_pixel(16, 0).0, PALETTE[0]);
        let stretched = render_frame("x", &shape("6x6u-bw", 64))?;
        let reader = png::Decoder::new(std::io::Cursor::new(&stretched.png)).read_info()?;
        assert_eq!(reader.info().color_type, png::ColorType::Rgb);
        assert!(matches!(
            render_frame("x", &shape("8x8u-bw", 0)),
            Err(ArchiveError::InvalidGeometry)
        ));
        Ok(())
    }

    #[test]
    fn sentence_and_nested_stopword_dim_use_specified_inks(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut shape = shape("8x8u-sent", 256);
        shape.stopword_dim = true;
        let frame = render_frame("x. y the \u{e}the fox\u{f} z", &shape)?;
        let rgb = image::load_from_memory(&frame.png)?.to_rgb8();
        let colors = |start: u32, end: u32| {
            rgb.enumerate_pixels()
                .filter(move |(x, y, _)| *x >= start && *x < end && *y < 8)
                .map(|(_, _, pixel)| pixel.0)
                .filter(|color| *color != PALETTE[0])
                .collect::<Vec<_>>()
        };
        assert!(colors(0, 16).iter().all(|color| *color == PALETTE[1]));
        assert!(colors(24, 32).contains(&PALETTE[2]));
        assert!(colors(40, 64).contains(&PALETTE[9]));
        assert!(colors(104, 128).iter().all(|color| *color == PALETTE[9]));
        assert!(colors(136, 144).contains(&PALETTE[2]));
        Ok(())
    }

    fn color_type(frame: &RenderedFrame) -> Result<png::ColorType, png::DecodingError> {
        let reader = png::Decoder::new(std::io::Cursor::new(&frame.png)).read_info()?;
        Ok(reader.info().color_type)
    }

    #[test]
    fn silver_draws_unicode_in_rgb_and_bitmap_shapes_fall_back_per_glyph(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let inked = |frame: &RenderedFrame, x: std::ops::Range<u32>, y: std::ops::Range<u32>| {
            let rgb = image::load_from_memory(&frame.png).map(|image| image.to_rgb8());
            rgb.map(|rgb| {
                rgb.enumerate_pixels()
                    .filter(|(px, py, pixel)| {
                        x.contains(px) && y.contains(py) && pixel.0 != PALETTE[0]
                    })
                    .count()
            })
        };
        let silver = render_frame("你", &shape("silver16-bw", 64))?;
        assert_eq!(color_type(&silver)?, png::ColorType::Rgb);
        assert!(inked(&silver, 0..16, 0..16)? > 0);
        assert_eq!(inked(&silver, 16..64, 0..64)?, 0);
        let missing = render_frame("\u{e000}", &shape("silver16-bw", 64))?;
        assert_eq!(inked(&missing, 0..64, 0..64)?, 0);
        assert_eq!(missing.chars, 1);
        let fallback = render_frame("你x", &shape("8on16-bw", 64))?;
        assert_eq!(color_type(&fallback)?, png::ColorType::Indexed);
        assert!(inked(&fallback, 0..16, 0..16)? > 0);
        assert!(inked(&fallback, 16..24, 0..16)? > 0);
        assert_eq!(inked(&fallback, 24..64, 0..64)?, 0);
        Ok(())
    }
}

//! Pixel canvas shared by bitmap and TrueType drawing, plus PNG encoders.
use super::ArchiveError;

pub(super) const WHITE: u8 = 0;
pub(super) const BAND: u8 = 8;
pub(super) const BLACK: u8 = 7;
pub(super) const DIM: u8 = 9;
pub(super) const PALETTE: [[u8; 3]; 10] = [
    [255, 255, 255],
    [109, 2, 2],
    [109, 53, 2],
    [24, 109, 2],
    [2, 109, 109],
    [2, 32, 109],
    [75, 2, 109],
    [0, 0, 0],
    [255, 247, 194],
    [128, 128, 128],
];

/// Palette-index canvas (natural bitmap layouts) or RGB canvas (Silver, stretched).
pub(super) struct Canvas {
    width: u32,
    height: u32,
    rgb: bool,
    data: Vec<u8>,
}

impl Canvas {
    pub(super) fn new(width: u32, height: u32, rgb: bool) -> Self {
        let pixels = width as usize * height as usize;
        let data = if rgb {
            vec![255; pixels * 3]
        } else {
            vec![WHITE; pixels]
        };
        Self {
            width,
            height,
            rgb,
            data,
        }
    }

    fn offset(&self, x: i64, y: i64) -> Option<usize> {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return None;
        }
        let index = usize::try_from(y).ok()? * self.width as usize + usize::try_from(x).ok()?;
        Some(if self.rgb { index * 3 } else { index })
    }

    pub(super) fn set(&mut self, x: i64, y: i64, color: u8) {
        let Some(at) = self.offset(x, y) else {
            return;
        };
        if self.rgb {
            self.data[at..at + 3].copy_from_slice(&PALETTE[usize::from(color)]);
        } else {
            self.data[at] = color;
        }
    }

    /// Fill a clipped rectangle with a palette color.
    pub(super) fn fill(&mut self, left: i64, top: i64, width: u32, height: u32, color: u8) {
        for y in top..top + i64::from(height) {
            for x in left..left + i64::from(width) {
                self.set(x, y, color);
            }
        }
    }

    /// Reset every row from `top` downward to white.
    pub(super) fn clear_from(&mut self, top: u32) {
        self.fill(
            0,
            i64::from(top),
            self.width,
            self.height.saturating_sub(top),
            WHITE,
        );
    }

    /// Draw one anti-aliased coverage sample in a palette color.
    ///
    /// RGB canvases alpha-blend over the existing pixel. Indexed canvases threshold:
    /// coverage of at least 170 takes the ink, black ink with coverage of at least 56 takes
    /// the dim fringe, and any other ink needs coverage of at least 110.
    pub(super) fn ink(&mut self, x: i64, y: i64, color: u8, coverage: u8) {
        if coverage == 0 {
            return;
        }
        let Some(at) = self.offset(x, y) else {
            return;
        };
        if self.rgb {
            let alpha = u32::from(coverage);
            for (channel, ink) in self.data[at..at + 3]
                .iter_mut()
                .zip(PALETTE[usize::from(color)])
            {
                let blended =
                    (u32::from(ink) * alpha + u32::from(*channel) * (255 - alpha) + 127) / 255;
                *channel = u8::try_from(blended).unwrap_or(u8::MAX);
            }
        } else if coverage >= 170 || (color != BLACK && coverage >= 110) {
            self.data[at] = color;
        } else if color == BLACK && coverage >= 56 {
            self.data[at] = DIM;
        }
    }

    pub(super) fn into_rgb(self) -> Vec<u8> {
        if self.rgb {
            self.data
        } else {
            self.data
                .iter()
                .flat_map(|&p| PALETTE[usize::from(p)])
                .collect()
        }
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, ArchiveError> {
        if self.rgb {
            encode_rgb(&self.data, self.width, self.height)
        } else {
            encode_indexed(&self.data, self.width, self.height)
        }
    }
}

pub(super) fn encode_indexed(
    pixels: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, ArchiveError> {
    let mut used = [false; 10];
    for &pixel in pixels {
        used[pixel as usize] = true;
    }
    let mut palette = Vec::new();
    let mut map = [0_u8; 10];
    let mut count = 0_u8;
    for (index, color) in PALETTE.iter().enumerate() {
        if used[index] {
            map[index] = count;
            count += 1;
            palette.extend_from_slice(color);
        }
    }
    let (bits, depth) = match count {
        0..=2 => (1, png::BitDepth::One),
        3..=4 => (2, png::BitDepth::Two),
        _ => (4, png::BitDepth::Four),
    };
    let stride = (width as usize * bits).div_ceil(8);
    let mut packed = vec![0_u8; stride * height as usize];
    for (row, source) in pixels.chunks_exact(width as usize).enumerate() {
        for (x, &pixel) in source.iter().enumerate() {
            packed[row * stride + x * bits / 8] |= map[pixel as usize] << (8 - bits - x * bits % 8);
        }
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(depth);
        encoder.set_palette(palette);
        encoder.set_compression(png::Compression::High);
        encoder.set_filter(png::Filter::NoFilter);
        let mut writer = encoder
            .write_header()
            .map_err(|e| ArchiveError::Encoding(e.to_string()))?;
        writer
            .write_image_data(&packed)
            .map_err(|e| ArchiveError::Encoding(e.to_string()))?;
    }
    Ok(bytes)
}

pub(super) fn encode_rgb(pixels: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ArchiveError> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::High);
        let mut writer = encoder
            .write_header()
            .map_err(|e| ArchiveError::Encoding(e.to_string()))?;
        writer
            .write_image_data(pixels)
            .map_err(|e| ArchiveError::Encoding(e.to_string()))?;
    }
    Ok(bytes)
}

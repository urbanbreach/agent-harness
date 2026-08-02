use harness_tui::UnwrapOrAbort;
use font8x8::unicode::{BasicFonts, BlockFonts, BoxFonts, LatinFonts, UnicodeFonts};
use fontdue::{Font, FontSettings};
use image::{imageops::FilterType, Rgb, RgbImage};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, OnceLock};
use vt100::{Color as VtColor, Parser as VtParser};

const GLYPH_WIDTH: u32 = 8;
const GLYPH_HEIGHT: u32 = 8;
const GLYPH_VERTICAL_SCALE: u32 = 2;
const DEFAULT_FG: [u8; 3] = [216, 216, 216];
const DEFAULT_BG: [u8; 3] = [20, 20, 20];
const ANTI_ALIAS_FONT_SIZE_FACTOR: f32 = 0.72;

type AntiAliasMask = Arc<Vec<u8>>;
type AntiAliasCache = RefCell<BTreeMap<(char, u32, bool), AntiAliasMask>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalRenderConfig {
    pub(crate) cell_width: u32,
    pub(crate) cell_height: u32,
    pub(crate) raster_scale: u32,
    pub(crate) cursor_position: Option<(u16, u16)>,
}

impl TerminalRenderConfig {
    pub(crate) const fn new(cell_width: u32, cell_height: u32) -> Self {
        Self {
            cell_width,
            cell_height,
            raster_scale: 4,
            cursor_position: None,
        }
    }

    fn raster_cell_metrics(self) -> RasterCellMetrics {
        RasterCellMetrics {
            width: GLYPH_WIDTH * self.raster_scale,
            height: GLYPH_HEIGHT * GLYPH_VERTICAL_SCALE * self.raster_scale,
            raster_scale: self.raster_scale,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RasterCellMetrics {
    width: u32,
    height: u32,
    raster_scale: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellBox {
    origin_x: u32,
    origin_y: u32,
    width: u32,
    height: u32,
}

impl CellBox {
    fn for_terminal_cell(row: u16, col: u16, metrics: RasterCellMetrics) -> Self {
        Self {
            origin_x: u32::from(col) * metrics.width,
            origin_y: u32::from(row) * metrics.height,
            width: metrics.width,
            height: metrics.height,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GlyphRenderSpec {
    cell: CellBox,
    ch: char,
    color: [u8; 3],
    bold: bool,
}

struct CellRenderContext<'a> {
    screen: &'a vt100::Screen,
    glyphs: &'a GlyphLookup,
    metrics: RasterCellMetrics,
    cursor_position: Option<(u16, u16)>,
}

pub(crate) fn render_parser_to_image(parser: &VtParser, config: TerminalRenderConfig) -> RgbImage {
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let metrics = config.raster_cell_metrics();
    let mut raster_image = RgbImage::new(
        u32::from(cols) * metrics.width,
        u32::from(rows) * metrics.height,
    );
    let glyphs = GlyphLookup::new();
    let context = CellRenderContext {
        screen,
        glyphs: &glyphs,
        metrics,
        cursor_position: config.cursor_position,
    };

    for row in 0..rows {
        for col in 0..cols {
            draw_cell(&mut raster_image, &context, row, col);
        }
    }

    let target_width = u32::from(cols) * config.cell_width;
    let target_height = u32::from(rows) * config.cell_height;
    if raster_image.width() == target_width && raster_image.height() == target_height {
        raster_image
    } else {
        image::imageops::resize(
            &raster_image,
            target_width,
            target_height,
            FilterType::Lanczos3,
        )
    }
}

pub(crate) fn extract_region_pixels(
    image: &RgbImage,
    region: (u16, u16, u16, u16),
    config: TerminalRenderConfig,
) -> Vec<u8> {
    let (row_start, col_start, height_cells, width_cells) = region;
    let x_start = u32::from(col_start) * config.cell_width;
    let y_start = u32::from(row_start) * config.cell_height;
    let width_px = u32::from(width_cells) * config.cell_width;
    let height_px = u32::from(height_cells) * config.cell_height;

    let mut data = Vec::with_capacity((width_px * height_px * 3) as usize);
    for y in y_start..(y_start + height_px) {
        for x in x_start..(x_start + width_px) {
            let [r, g, b] = image.get_pixel(x, y).0;
            data.push(r);
            data.push(g);
            data.push(b);
        }
    }

    data
}

pub(crate) fn parse_ttf_antialias_env(value: Option<&str>) -> bool {
    value
        .map(|value| {
            let normalized = value.trim();
            !(normalized == "0"
                || normalized.eq_ignore_ascii_case("false")
                || normalized.eq_ignore_ascii_case("off"))
        })
        .unwrap_or(true)
}

fn draw_cell(image: &mut RgbImage, context: &CellRenderContext<'_>, row: u16, col: u16) {
    let cell_box = CellBox::for_terminal_cell(row, col, context.metrics);
    let Some(cell) = context.screen.cell(row, col) else {
        fill_cell_background(image, cell_box, DEFAULT_BG);
        return;
    };

    let cursor_over_cell = context.cursor_position == Some((row, col));
    let mut fg = terminal_color_to_rgb(cell.fgcolor(), true);
    let mut bg = terminal_color_to_rgb(cell.bgcolor(), false);
    if cell.inverse() && !cursor_over_cell {
        std::mem::swap(&mut fg, &mut bg);
    }
    let bold = cell.bold();
    if bold {
        fg = brighten(fg, 28);
    }

    fill_cell_background(image, cell_box, bg);
    if cell.is_wide_continuation() {
        return;
    }

    let Some(ch) = cell.contents().chars().next() else {
        return;
    };

    draw_cell_glyph(
        image,
        context.glyphs,
        context.metrics,
        GlyphRenderSpec {
            cell: cell_box,
            ch,
            color: fg,
            bold,
        },
    );
    if cell.underline() {
        draw_underline(image, cell_box, fg, context.metrics.raster_scale);
    }
}

fn fill_cell_background(image: &mut RgbImage, cell: CellBox, color: [u8; 3]) {
    for y in 0..cell.height {
        for x in 0..cell.width {
            image.put_pixel(cell.origin_x + x, cell.origin_y + y, Rgb(color));
        }
    }
}

fn draw_cell_glyph(
    image: &mut RgbImage,
    glyphs: &GlyphLookup,
    metrics: RasterCellMetrics,
    spec: GlyphRenderSpec,
) {
    if spec.ch == ' ' {
        return;
    }

    if ttf_antialias_enabled()
        && !prefers_bitmap_terminal_glyph(spec.ch)
        && glyphs.draw_antialiased_glyph(image, metrics, spec)
    {
        return;
    }

    let glyph = glyphs
        .glyph(spec.ch)
        .or_else(|| glyphs.glyph('?'))
        .unwrap_or([0_u8; 8]);

    for (glyph_row, row_bits) in glyph.into_iter().enumerate() {
        let glyph_row = u32::try_from(glyph_row).unwrap_or_abort();
        for bit in 0_u8..8 {
            let pixel_is_on = row_bits & (1_u8 << bit) != 0;
            if !pixel_is_on {
                continue;
            }

            let pixel_x = spec.cell.origin_x + u32::from(bit) * metrics.raster_scale;
            let pixel_y =
                spec.cell.origin_y + glyph_row * GLYPH_VERTICAL_SCALE * metrics.raster_scale;
            for y in 0..(GLYPH_VERTICAL_SCALE * metrics.raster_scale) {
                for x in 0..metrics.raster_scale {
                    image.put_pixel(pixel_x + x, pixel_y + y, Rgb(spec.color));
                }
            }
        }
    }
}

fn prefers_bitmap_terminal_glyph(ch: char) -> bool {
    let code = ch as u32;
    (0x2500..=0x259F).contains(&code)
}

fn draw_underline(image: &mut RgbImage, cell: CellBox, color: [u8; 3], raster_scale: u32) {
    let thickness = raster_scale.max(1);
    let y_start = cell.origin_y + cell.height.saturating_sub(thickness);
    for y in 0..thickness {
        for x in 0..cell.width {
            image.put_pixel(cell.origin_x + x, y_start + y, Rgb(color));
        }
    }
}

fn terminal_color_to_rgb(color: VtColor, foreground: bool) -> [u8; 3] {
    match color {
        VtColor::Default => {
            if foreground {
                DEFAULT_FG
            } else {
                DEFAULT_BG
            }
        }
        VtColor::Idx(idx) => xterm_256_color(idx),
        VtColor::Rgb(r, g, b) => [r, g, b],
    }
}

fn xterm_256_color(idx: u8) -> [u8; 3] {
    const ANSI_16: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [255, 255, 255],
    ];

    match idx {
        0..=15 => ANSI_16[usize::from(idx)],
        16..=231 => {
            let value = idx - 16;
            let r = value / 36;
            let g = (value % 36) / 6;
            let b = value % 6;
            [cube_level(r), cube_level(g), cube_level(b)]
        }
        232..=255 => {
            let gray = 8_u8.saturating_add((idx - 232) * 10);
            [gray, gray, gray]
        }
    }
}

fn cube_level(level: u8) -> u8 {
    if level == 0 {
        0
    } else {
        level.saturating_mul(40).saturating_add(55)
    }
}

fn brighten(color: [u8; 3], amount: u8) -> [u8; 3] {
    [
        color[0].saturating_add(amount),
        color[1].saturating_add(amount),
        color[2].saturating_add(amount),
    ]
}

struct GlyphLookup {
    basic: BasicFonts,
    latin: LatinFonts,
    box_drawing: BoxFonts,
    block: BlockFonts,
    smooth_font: Option<Font>,
    bold_smooth_font: Option<Font>,
    anti_alias_cache: AntiAliasCache,
}

impl GlyphLookup {
    fn new() -> Self {
        Self {
            basic: BasicFonts::new(),
            latin: LatinFonts::new(),
            box_drawing: BoxFonts::new(),
            block: BlockFonts::new(),
            smooth_font: load_anti_alias_font(),
            bold_smooth_font: load_anti_alias_bold_font(),
            anti_alias_cache: RefCell::new(BTreeMap::new()),
        }
    }

    fn draw_antialiased_glyph(
        &self,
        image: &mut RgbImage,
        metrics: RasterCellMetrics,
        spec: GlyphRenderSpec,
    ) -> bool {
        let Some(mask) = self.anti_alias_mask_for(spec.ch, metrics, spec.bold) else {
            return false;
        };

        for y in 0..spec.cell.height {
            for x in 0..spec.cell.width {
                let idx = usize::try_from(y * spec.cell.width + x).unwrap_or_abort();
                let alpha = mask[idx];
                if alpha == 0 {
                    continue;
                }

                let pixel = image.get_pixel_mut(spec.cell.origin_x + x, spec.cell.origin_y + y);
                let [r, g, b] = pixel.0;
                let a = u16::from(alpha);
                let inv = 255_u16.saturating_sub(a);
                pixel.0 = [
                    ((u16::from(r) * inv + u16::from(spec.color[0]) * a + 127) / 255) as u8,
                    ((u16::from(g) * inv + u16::from(spec.color[1]) * a + 127) / 255) as u8,
                    ((u16::from(b) * inv + u16::from(spec.color[2]) * a + 127) / 255) as u8,
                ];
            }
        }

        true
    }

    fn anti_alias_mask_for(
        &self,
        ch: char,
        metrics: RasterCellMetrics,
        bold: bool,
    ) -> Option<AntiAliasMask> {
        let cache_key = (ch, metrics.raster_scale, bold);
        if let Some(mask) = self.anti_alias_cache.borrow().get(&cache_key) {
            return Some(mask.clone());
        }

        let font = if bold {
            self.bold_smooth_font
                .as_ref()
                .or(self.smooth_font.as_ref())?
        } else {
            self.smooth_font.as_ref()?
        };
        let font_size = (metrics.height as f32 * ANTI_ALIAS_FONT_SIZE_FACTOR).max(8.0);
        let line_metrics = font.horizontal_line_metrics(font_size);
        let reference_metrics = font.metrics('M', font_size);
        let baseline_origin_x = ((metrics.width as f32 - reference_metrics.advance_width).max(0.0)
            * 0.5)
            .round() as i32;
        let baseline_y = line_metrics
            .map(|line| {
                let centered_top =
                    ((metrics.height as f32 - line.new_line_size).max(0.0) * 0.5).round();
                (centered_top + line.ascent).round() as i32
            })
            .unwrap_or_else(|| {
                i32::try_from(metrics.height / 2).unwrap_or_abort()
            });

        let (glyph_metrics, bitmap) = font.rasterize(ch, font_size);
        let mut mask = vec![0_u8; (metrics.width * metrics.height) as usize];
        if glyph_metrics.width == 0 || glyph_metrics.height == 0 {
            let mask = Arc::new(mask);
            self.anti_alias_cache
                .borrow_mut()
                .insert(cache_key, mask.clone());
            return Some(mask);
        }

        let glyph_width = u32::try_from(glyph_metrics.width).unwrap_or_abort();
        let glyph_height = u32::try_from(glyph_metrics.height).unwrap_or_abort();
        let x_offset = baseline_origin_x + glyph_metrics.xmin;
        let y_offset = baseline_y
            - glyph_metrics.ymin
            - i32::try_from(glyph_height).unwrap_or_abort();

        for y in 0..glyph_height {
            for x in 0..glyph_width {
                let src_idx = usize::try_from(y * glyph_width + x).unwrap_or_abort();
                let alpha = bitmap[src_idx];
                if alpha == 0 {
                    continue;
                }

                let dx = x_offset + i32::try_from(x).unwrap_or_abort();
                let dy = y_offset + i32::try_from(y).unwrap_or_abort();
                if dx < 0 || dy < 0 {
                    continue;
                }

                let dx = u32::try_from(dx).unwrap_or_abort();
                let dy = u32::try_from(dy).unwrap_or_abort();
                if dx >= metrics.width || dy >= metrics.height {
                    continue;
                }

                let dst_idx =
                    usize::try_from(dy * metrics.width + dx).unwrap_or_abort();
                mask[dst_idx] = alpha;
            }
        }

        let mask = Arc::new(mask);
        self.anti_alias_cache
            .borrow_mut()
            .insert(cache_key, mask.clone());
        Some(mask)
    }

    fn glyph(&self, ch: char) -> Option<[u8; 8]> {
        self.basic
            .get(ch)
            .or_else(|| self.latin.get(ch))
            .or_else(|| self.box_drawing.get(ch))
            .or_else(|| self.block.get(ch))
    }
}

fn load_anti_alias_font() -> Option<Font> {
    if !ttf_antialias_enabled() {
        return None;
    }

    load_font_from_candidates(font_candidates(
        "HARNESS_VISUAL_FONT_PATH",
        "monospace",
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
        ],
    ))
}

fn load_anti_alias_bold_font() -> Option<Font> {
    if !ttf_antialias_enabled() {
        return None;
    }

    let mut candidates = font_candidates(
        "HARNESS_VISUAL_FONT_BOLD_PATH",
        "monospace:style=Bold",
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
            "/usr/share/fonts/dejavu/DejaVuSansMono-Bold.ttf",
        ],
    );
    if let Ok(path) = std::env::var("HARNESS_VISUAL_FONT_PATH") {
        if let Some(derived) = bold_font_candidate_from_regular(&path) {
            let insert_idx = usize::from(std::env::var("HARNESS_VISUAL_FONT_BOLD_PATH").is_ok());
            candidates.insert(insert_idx, derived);
        }
    }
    load_font_from_candidates(candidates)
}

fn load_font_from_candidates(candidates: Vec<String>) -> Option<Font> {
    for path in candidates {
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) else {
            continue;
        };
        return Some(font);
    }

    None
}

fn font_candidates(
    env_var: &str,
    fontconfig_pattern: &str,
    fallback_paths: &[&str],
) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var(env_var) {
        candidates.push(path);
    }
    candidates.extend(fallback_paths.iter().map(|path| (*path).to_string()));
    if let Some(path) = fontconfig_match(fontconfig_pattern) {
        if !candidates.iter().any(|candidate| candidate == &path) {
            candidates.push(path);
        }
    }
    candidates
}

fn fontconfig_match(pattern: &str) -> Option<String> {
    let output = Command::new("fc-match")
        .args(["-f", "%{file}\n", pattern])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn bold_font_candidate_from_regular(path: &str) -> Option<String> {
    let path = Path::new(path);
    let stem = path.file_stem()?.to_str()?;
    let ext = path.extension()?.to_str()?;
    Some(
        path.with_file_name(format!("{stem}-Bold.{ext}"))
            .to_string_lossy()
            .into_owned(),
    )
}

fn ttf_antialias_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        parse_ttf_antialias_env(
            std::env::var("HARNESS_VISUAL_TTF_ANTIALIAS")
                .ok()
                .as_deref(),
        )
    })
}

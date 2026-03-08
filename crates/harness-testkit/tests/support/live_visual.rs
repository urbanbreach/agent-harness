use font8x8::unicode::{BasicFonts, BlockFonts, BoxFonts, LatinFonts, UnicodeFonts};
use fontdue::{Font, FontSettings};
use image::{imageops::FilterType, Rgb, RgbImage};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;
use time::{macros::format_description, OffsetDateTime};
use vt100::{Color as VtColor, Parser as VtParser};

pub const CHECKPOINT_STARTUP: &str = "startup";
pub const CHECKPOINT_PERMISSION_REQUESTED: &str = "permission_requested";
pub const CHECKPOINT_PROMPT_STREAM: &str = "prompt_stream";
pub const CHECKPOINT_DRAFT_VISIBLE: &str = "draft_visible";
pub const CHECKPOINT_SHELL_CREATE_FINISHED: &str = "shell_create_finished";
pub const CHECKPOINT_HASHLINE_SCAN_FINISHED: &str = "hashline_scan_finished";
pub const CHECKPOINT_RUN_FINISHED: &str = "run_finished";

const LIVE_PROXY_NAMESPACE: &str = "live-proxy";
const DEFAULT_LIVE_VISUAL_RETENTION_RUNS: usize = 5;
const GLYPH_WIDTH: u32 = 8;
const GLYPH_HEIGHT: u32 = 8;
const GLYPH_VERTICAL_SCALE: u32 = 2;
const RASTER_SCALE: u32 = 4;
const RASTER_CELL_WIDTH: u32 = GLYPH_WIDTH * RASTER_SCALE;
const RASTER_CELL_HEIGHT: u32 = GLYPH_HEIGHT * GLYPH_VERTICAL_SCALE * RASTER_SCALE;
const DEFAULT_FG: [u8; 3] = [216, 216, 216];
const DEFAULT_BG: [u8; 3] = [18, 18, 18];
const ANTI_ALIAS_FONT_SIZE_FACTOR: f32 = 0.72;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveViewportPreset {
    pub name: &'static str,
    pub rows: u16,
    pub cols: u16,
    pub cell_width: u32,
    pub cell_height: u32,
}

impl LiveViewportPreset {
    pub const DESKTOP: Self = Self {
        name: "desktop",
        rows: 48,
        cols: 160,
        cell_width: 16,
        cell_height: 30,
    };

    pub const LAPTOP: Self = Self {
        name: "laptop",
        rows: 42,
        cols: 140,
        cell_width: 16,
        cell_height: 30,
    };

    pub const COMPACT: Self = Self {
        name: "compact",
        rows: 36,
        cols: 120,
        cell_width: 16,
        cell_height: 30,
    };
}

pub fn selected_live_viewport() -> LiveViewportPreset {
    match std::env::var("HARNESS_LIVE_VISUAL_VIEWPORT") {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "compact" => LiveViewportPreset::COMPACT,
            "laptop" => LiveViewportPreset::LAPTOP,
            "desktop" | "" => LiveViewportPreset::DESKTOP,
            _ => LiveViewportPreset::DESKTOP,
        },
        Err(_) => LiveViewportPreset::DESKTOP,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusCapture {
    marker: String,
    width_cells: u16,
    height_cells: u16,
    top_padding_cells: u16,
    left_padding_cells: u16,
}

impl FocusCapture {
    pub fn anchored(marker: impl Into<String>, width_cells: u16, height_cells: u16) -> Self {
        Self {
            marker: marker.into(),
            width_cells,
            height_cells,
            top_padding_cells: 2,
            left_padding_cells: 2,
        }
    }

    pub fn anchored_exact(marker: impl Into<String>, width_cells: u16, height_cells: u16) -> Self {
        Self {
            marker: marker.into(),
            width_cells,
            height_cells,
            top_padding_cells: 0,
            left_padding_cells: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveVisualCheckpoint {
    png_path: PathBuf,
    manifest_json_path: PathBuf,
    manifest_jsonl_path: PathBuf,
    focus_marker_found: bool,
    focus_region_cells: (u16, u16, u16, u16),
}

impl LiveVisualCheckpoint {
    pub fn png_path(&self) -> &Path {
        &self.png_path
    }

    pub fn manifest_json_path(&self) -> &Path {
        &self.manifest_json_path
    }

    pub fn manifest_jsonl_path(&self) -> &Path {
        &self.manifest_jsonl_path
    }

    pub fn focus_marker_found(&self) -> bool {
        self.focus_marker_found
    }

    pub fn focus_region_cells(&self) -> (u16, u16, u16, u16) {
        self.focus_region_cells
    }
}

#[derive(Debug, Clone)]
pub struct LiveVisualRunOptions {
    pub run_metadata: Value,
}

impl Default for LiveVisualRunOptions {
    fn default() -> Self {
        Self {
            run_metadata: Value::Object(serde_json::Map::new()),
        }
    }
}

#[derive(Debug)]
pub struct LiveVisualRun {
    run_dir: PathBuf,
    manifest: VisualManifest,
}

impl LiveVisualRun {
    pub fn new(test_name: &str, run_id: &str) -> Result<Self, String> {
        Self::new_in_with_options(
            visual_artifacts_root(),
            test_name,
            run_id,
            LiveVisualRunOptions::default(),
        )
    }

    pub fn new_in(root: PathBuf, test_name: &str, run_id: &str) -> Result<Self, String> {
        Self::new_in_with_options(root, test_name, run_id, LiveVisualRunOptions::default())
    }

    pub fn new_with_options(
        test_name: &str,
        run_id: &str,
        options: LiveVisualRunOptions,
    ) -> Result<Self, String> {
        Self::new_in_with_options(visual_artifacts_root(), test_name, run_id, options)
    }

    pub fn new_in_with_options(
        root: PathBuf,
        test_name: &str,
        run_id: &str,
        options: LiveVisualRunOptions,
    ) -> Result<Self, String> {
        if test_name.trim().is_empty() {
            return Err("live visual test name cannot be empty".to_string());
        }
        if run_id.trim().is_empty() {
            return Err("live visual run id cannot be empty".to_string());
        }

        let test_root = root.join(LIVE_PROXY_NAMESPACE).join(test_name);
        fs::create_dir_all(&test_root).map_err(|err| {
            format!(
                "failed to create live visual test root {}: {err}",
                test_root.display()
            )
        })?;
        prune_old_live_visual_runs(&test_root, run_id)?;

        let run_dir = test_root.join(run_id);
        fs::create_dir_all(&run_dir).map_err(|err| {
            format!(
                "failed to create live visual dir {}: {err}",
                run_dir.display()
            )
        })?;

        Ok(Self {
            manifest: VisualManifest::new_in_with_metadata(
                run_dir.clone(),
                test_name,
                run_id,
                options.run_metadata,
            )?,
            run_dir,
        })
    }

    pub fn capture_checkpoint(
        &mut self,
        checkpoint_id: &str,
        parser: &VtParser,
        screen_markers: &[&str],
        focus: &FocusCapture,
    ) -> Result<LiveVisualCheckpoint, String> {
        self.capture_checkpoint_with_metadata(checkpoint_id, parser, screen_markers, focus, None)
    }

    pub fn capture_checkpoint_with_metadata(
        &mut self,
        checkpoint_id: &str,
        parser: &VtParser,
        screen_markers: &[&str],
        focus: &FocusCapture,
        metadata: Option<Value>,
    ) -> Result<LiveVisualCheckpoint, String> {
        ensure_known_checkpoint_id(checkpoint_id)?;

        let image = render_parser_to_image(parser);
        let file_name = format!("live_proxy_{checkpoint_id}.png");
        let png_path = self.run_dir.join(&file_name);
        image
            .save(&png_path)
            .map_err(|err| format!("failed to save {}: {err}", png_path.display()))?;

        let (rows, cols) = parser.screen().size();
        let focus_region = find_marker_cell(parser.screen(), &focus.marker).map(|(row, col)| {
            anchored_region(
                row,
                col,
                rows,
                cols,
                focus.width_cells,
                focus.height_cells,
                focus.top_padding_cells,
                focus.left_padding_cells,
            )
        });
        let focus_marker_found = focus_region.is_some();
        let focus_region_cells = focus_region.unwrap_or((0, 0, rows.max(1), cols.max(1)));
        let focus_pixels = extract_region_pixels(&image, focus_region_cells);
        let marker_states =
            marker_presence_states(parser.screen().contents().as_str(), screen_markers);
        let focus_scope = if focus_marker_found {
            "anchored"
        } else {
            "full_frame_fallback"
        };
        let focus_pixels_blake3 = blake3::hash(&focus_pixels).to_hex().to_string();
        let manifest_entry = VisualManifestEntry::new(VisualManifestEntrySpec {
            checkpoint_id,
            captured_at_stage: checkpoint_id,
            png_path: &png_path,
            file_name: &file_name,
            screen_markers: &marker_states,
            focus_marker: &focus.marker,
            focus_marker_found,
            focus_scope,
            focus_pixels_blake3: &focus_pixels_blake3,
            focus_region_cells,
            image_size: (image.width(), image.height()),
            metadata: metadata.as_ref(),
        })?;

        self.manifest.append_checkpoint(manifest_entry)?;

        Ok(LiveVisualCheckpoint {
            png_path,
            manifest_json_path: self.manifest.manifest_json_path().to_path_buf(),
            manifest_jsonl_path: self.manifest.manifest_jsonl_path().to_path_buf(),
            focus_marker_found,
            focus_region_cells,
        })
    }

    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }
}

#[derive(Debug, Clone)]
pub struct VisualManifestEntry {
    checkpoint_id: String,
    captured_at_stage: String,
    value: Value,
}

#[derive(Debug, Clone)]
pub struct VisualManifestEntrySpec<'a> {
    pub checkpoint_id: &'a str,
    pub captured_at_stage: &'a str,
    pub png_path: &'a Path,
    pub file_name: &'a str,
    pub screen_markers: &'a [(String, bool)],
    pub focus_marker: &'a str,
    pub focus_marker_found: bool,
    pub focus_scope: &'a str,
    pub focus_pixels_blake3: &'a str,
    pub focus_region_cells: (u16, u16, u16, u16),
    pub image_size: (u32, u32),
    pub metadata: Option<&'a Value>,
}

impl VisualManifestEntry {
    pub fn new(spec: VisualManifestEntrySpec<'_>) -> Result<Self, String> {
        ensure_known_checkpoint_id(spec.checkpoint_id)?;
        ensure_known_checkpoint_id(spec.captured_at_stage)?;

        Ok(Self {
            checkpoint_id: spec.checkpoint_id.to_string(),
            captured_at_stage: spec.captured_at_stage.to_string(),
            value: json!({
                "checkpoint_id": spec.checkpoint_id,
                "png_path": spec.png_path.display().to_string(),
                "screen_markers": spec.screen_markers.iter().map(|(marker, present)| {
                    json!({
                        "marker": marker,
                        "present": present,
                    })
                }).collect::<Vec<_>>(),
                "captured_at_stage": spec.captured_at_stage,
                "focus": {
                    "marker": spec.focus_marker,
                    "found": spec.focus_marker_found,
                    "scope": spec.focus_scope,
                    "pixels_blake3": spec.focus_pixels_blake3,
                },
                "region": {
                    "row": spec.focus_region_cells.0,
                    "col": spec.focus_region_cells.1,
                    "height": spec.focus_region_cells.2,
                    "width": spec.focus_region_cells.3,
                },
                "image": {
                    "file_name": spec.file_name,
                    "size_px": {
                        "width": spec.image_size.0,
                        "height": spec.image_size.1,
                    }
                },
                "metadata": spec.metadata.cloned().unwrap_or(Value::Null),
            }),
        })
    }

    fn checkpoint_id(&self) -> &str {
        &self.checkpoint_id
    }

    fn captured_at_stage(&self) -> &str {
        &self.captured_at_stage
    }

    fn as_value(&self) -> &Value {
        &self.value
    }
}

#[derive(Debug)]
pub struct VisualManifest {
    test_name: String,
    run_id: String,
    run_metadata: Value,
    manifest_json_path: PathBuf,
    manifest_jsonl_path: PathBuf,
    entries: Vec<VisualManifestEntry>,
}

impl VisualManifest {
    pub fn new_in(output_dir: PathBuf, test_name: &str, run_id: &str) -> Result<Self, String> {
        Self::new_in_with_metadata(
            output_dir,
            test_name,
            run_id,
            Value::Object(serde_json::Map::new()),
        )
    }

    pub fn new_in_with_metadata(
        output_dir: PathBuf,
        test_name: &str,
        run_id: &str,
        run_metadata: Value,
    ) -> Result<Self, String> {
        if test_name.trim().is_empty() {
            return Err("live visual test name cannot be empty".to_string());
        }
        if run_id.trim().is_empty() {
            return Err("live visual run id cannot be empty".to_string());
        }

        fs::create_dir_all(&output_dir).map_err(|err| {
            format!(
                "failed to create live visual dir {}: {err}",
                output_dir.display()
            )
        })?;

        let manifest_json_path = output_dir.join("manifest.json");
        let manifest_jsonl_path = output_dir.join("manifest.jsonl");
        let entries = load_existing_entries(&manifest_json_path)?;

        Ok(Self {
            test_name: test_name.to_string(),
            run_id: run_id.to_string(),
            run_metadata,
            manifest_json_path,
            manifest_jsonl_path,
            entries,
        })
    }

    pub fn append_checkpoint(&mut self, entry: VisualManifestEntry) -> Result<(), String> {
        if self
            .entries
            .iter()
            .any(|existing| existing.checkpoint_id() == entry.checkpoint_id())
        {
            return Err(format!(
                "duplicate visual checkpoint id `{}` in manifest",
                entry.checkpoint_id()
            ));
        }
        if self
            .entries
            .iter()
            .any(|existing| existing.captured_at_stage() == entry.captured_at_stage())
        {
            return Err(format!(
                "duplicate visual stage id `{}` in manifest",
                entry.captured_at_stage()
            ));
        }

        self.entries.push(entry);
        self.entries
            .sort_by_key(|item| checkpoint_order(item.captured_at_stage()));
        self.persist()
    }

    pub fn manifest_json_path(&self) -> &Path {
        &self.manifest_json_path
    }

    pub fn manifest_jsonl_path(&self) -> &Path {
        &self.manifest_jsonl_path
    }

    fn persist(&self) -> Result<(), String> {
        let manifest = json!({
            "test_name": self.test_name,
            "run_id": self.run_id,
            "run_metadata": self.run_metadata.clone(),
            "checkpoints": self.entries.iter().map(VisualManifestEntry::as_value).collect::<Vec<_>>(),
        });
        let rendered = serde_json::to_string_pretty(&manifest)
            .map_err(|err| format!("failed to serialize live visual manifest JSON: {err}"))?;
        fs::write(&self.manifest_json_path, rendered).map_err(|err| {
            format!(
                "failed to write live visual manifest {}: {err}",
                self.manifest_json_path.display()
            )
        })?;

        let jsonl = self
            .entries
            .iter()
            .map(|entry| entry.as_value().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let jsonl = if jsonl.is_empty() {
            jsonl
        } else {
            format!("{jsonl}\n")
        };
        fs::write(&self.manifest_jsonl_path, jsonl).map_err(|err| {
            format!(
                "failed to write live visual manifest {}: {err}",
                self.manifest_jsonl_path.display()
            )
        })
    }
}

fn load_existing_entries(manifest_json_path: &Path) -> Result<Vec<VisualManifestEntry>, String> {
    if !manifest_json_path.exists() {
        return Ok(Vec::new());
    }

    let manifest_text = fs::read_to_string(manifest_json_path).map_err(|err| {
        format!(
            "failed to read live visual manifest {}: {err}",
            manifest_json_path.display()
        )
    })?;
    let manifest: Value = serde_json::from_str(&manifest_text)
        .map_err(|err| format!("failed to parse live visual manifest JSON: {err}"))?;
    let checkpoints = manifest
        .get("checkpoints")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    checkpoints
        .into_iter()
        .map(|value| {
            let checkpoint_id = value
                .get("checkpoint_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "manifest checkpoint missing checkpoint_id".to_string())?;
            let captured_at_stage = value
                .get("captured_at_stage")
                .and_then(Value::as_str)
                .ok_or_else(|| "manifest checkpoint missing captured_at_stage".to_string())?;
            ensure_known_checkpoint_id(checkpoint_id)?;
            ensure_known_checkpoint_id(captured_at_stage)?;
            Ok(VisualManifestEntry {
                checkpoint_id: checkpoint_id.to_string(),
                captured_at_stage: captured_at_stage.to_string(),
                value,
            })
        })
        .collect()
}

fn ensure_known_checkpoint_id(checkpoint_id: &str) -> Result<(), String> {
    match checkpoint_id {
        CHECKPOINT_STARTUP
        | CHECKPOINT_PERMISSION_REQUESTED
        | CHECKPOINT_PROMPT_STREAM
        | CHECKPOINT_DRAFT_VISIBLE
        | CHECKPOINT_SHELL_CREATE_FINISHED
        | CHECKPOINT_HASHLINE_SCAN_FINISHED
        | CHECKPOINT_RUN_FINISHED => Ok(()),
        other => Err(format!("unknown live visual checkpoint id `{other}`")),
    }
}

fn checkpoint_order(checkpoint_id: &str) -> usize {
    match checkpoint_id {
        CHECKPOINT_STARTUP => 0,
        CHECKPOINT_DRAFT_VISIBLE => 1,
        CHECKPOINT_PERMISSION_REQUESTED => 2,
        CHECKPOINT_SHELL_CREATE_FINISHED => 3,
        CHECKPOINT_HASHLINE_SCAN_FINISHED => 4,
        CHECKPOINT_PROMPT_STREAM => 5,
        CHECKPOINT_RUN_FINISHED => 6,
        _ => usize::MAX,
    }
}

fn marker_presence_states(screen: &str, markers: &[&str]) -> Vec<(String, bool)> {
    markers
        .iter()
        .map(|marker| ((*marker).to_string(), screen.contains(marker)))
        .collect()
}

pub fn assert_checkpoint_markers(
    manifest_json_path: &Path,
    checkpoint_id: &str,
    required_present: &[&str],
    required_absent: &[&str],
) -> Result<(), String> {
    let manifest_text = fs::read_to_string(manifest_json_path).map_err(|err| {
        format!(
            "failed to read live visual manifest {}: {err}",
            manifest_json_path.display()
        )
    })?;
    let manifest: Value = serde_json::from_str(&manifest_text)
        .map_err(|err| format!("failed to parse live visual manifest JSON: {err}"))?;
    let checkpoint = manifest
        .get("checkpoints")
        .and_then(Value::as_array)
        .and_then(|entries| {
            entries.iter().find(|entry| {
                entry
                    .get("checkpoint_id")
                    .and_then(Value::as_str)
                    .map(|value| value == checkpoint_id)
                    .unwrap_or(false)
            })
        })
        .ok_or_else(|| {
            format!(
                "checkpoint `{checkpoint_id}` missing from {}",
                manifest_json_path.display()
            )
        })?;
    let marker_map = checkpoint
        .get("screen_markers")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("checkpoint `{checkpoint_id}` missing screen_markers"))?
        .iter()
        .filter_map(|entry| {
            Some((
                entry.get("marker")?.as_str()?.to_string(),
                entry.get("present")?.as_bool()?,
            ))
        })
        .collect::<BTreeMap<_, _>>();

    for marker in required_present {
        match marker_map.get(*marker) {
            Some(true) => {}
            Some(false) => {
                return Err(format!(
                    "checkpoint `{checkpoint_id}` expected marker `{marker}` to be present"
                ));
            }
            None => {
                return Err(format!(
                    "checkpoint `{checkpoint_id}` did not record marker `{marker}`"
                ));
            }
        }
    }

    for marker in required_absent {
        match marker_map.get(*marker) {
            Some(false) => {}
            Some(true) => {
                return Err(format!(
                    "checkpoint `{checkpoint_id}` expected marker `{marker}` to be absent"
                ));
            }
            None => {
                return Err(format!(
                    "checkpoint `{checkpoint_id}` did not record marker `{marker}`"
                ));
            }
        }
    }

    Ok(())
}

fn visual_artifacts_root() -> PathBuf {
    if let Ok(dir) = std::env::var("HARNESS_VISUAL_ARTIFACT_DIR") {
        return PathBuf::from(dir);
    }

    repo_root().join("target").join("pty-visual-artifacts")
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("harness-testkit should live under <repo>/crates/harness-testkit")
}

fn find_marker_cell(screen: &vt100::Screen, marker: &str) -> Option<(u16, u16)> {
    let (rows, cols) = screen.size();

    for row in 0..rows {
        let text = (0..cols)
            .filter_map(|col| screen.cell(row, col))
            .map(|cell| {
                let glyph = cell.contents();
                if glyph.is_empty() {
                    " ".to_string()
                } else {
                    glyph
                }
            })
            .collect::<String>();

        if let Some(byte_idx) = text.find(marker) {
            let col = text[..byte_idx].chars().count();
            if let Ok(col) = u16::try_from(col) {
                return Some((row, col));
            }
        }
    }

    None
}

fn anchored_region(
    anchor_row: u16,
    anchor_col: u16,
    rows: u16,
    cols: u16,
    width_cells: u16,
    height_cells: u16,
    top_padding_cells: u16,
    left_padding_cells: u16,
) -> (u16, u16, u16, u16) {
    let row_start = anchor_row.saturating_sub(top_padding_cells);
    let col_start = anchor_col.saturating_sub(left_padding_cells);

    let max_height = rows.saturating_sub(row_start).max(1);
    let max_width = cols.saturating_sub(col_start).max(1);

    let height = height_cells.min(max_height).max(1);
    let width = width_cells.min(max_width).max(1);

    (row_start, col_start, height, width)
}

fn extract_region_pixels(image: &RgbImage, region: (u16, u16, u16, u16)) -> Vec<u8> {
    let (row_start, col_start, height_cells, width_cells) = region;
    let viewport = selected_live_viewport();

    let x_start = u32::from(col_start) * viewport.cell_width;
    let y_start = u32::from(row_start) * viewport.cell_height;
    let width_px = u32::from(width_cells) * viewport.cell_width;
    let height_px = u32::from(height_cells) * viewport.cell_height;

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

fn render_parser_to_image(parser: &VtParser) -> RgbImage {
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let viewport = selected_live_viewport();
    let mut raster_image = RgbImage::new(
        u32::from(cols) * RASTER_CELL_WIDTH,
        u32::from(rows) * RASTER_CELL_HEIGHT,
    );
    let glyphs = GlyphLookup::new();
    let cursor_position = None;

    for row in 0..rows {
        for col in 0..cols {
            draw_cell(
                &mut raster_image,
                screen,
                row,
                col,
                &glyphs,
                cursor_position,
                RASTER_SCALE,
            );
        }
    }

    let target_width = u32::from(cols) * viewport.cell_width;
    let target_height = u32::from(rows) * viewport.cell_height;
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

fn draw_cell(
    image: &mut RgbImage,
    screen: &vt100::Screen,
    row: u16,
    col: u16,
    glyphs: &GlyphLookup,
    cursor_position: Option<(u16, u16)>,
    raster_scale: u32,
) {
    let cell_width = GLYPH_WIDTH * raster_scale;
    let cell_height = GLYPH_HEIGHT * GLYPH_VERTICAL_SCALE * raster_scale;
    let origin_x = u32::from(col) * cell_width;
    let origin_y = u32::from(row) * cell_height;
    let Some(cell) = screen.cell(row, col) else {
        fill_cell_background(
            image,
            origin_x,
            origin_y,
            DEFAULT_BG,
            cell_width,
            cell_height,
        );
        return;
    };

    let cursor_over_cell = cursor_position == Some((row, col));
    let mut fg = terminal_color_to_rgb(cell.fgcolor(), true);
    let mut bg = terminal_color_to_rgb(cell.bgcolor(), false);
    if cell.inverse() && !cursor_over_cell {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.bold() {
        fg = brighten(fg, 28);
    }

    fill_cell_background(image, origin_x, origin_y, bg, cell_width, cell_height);
    if cell.is_wide_continuation() {
        return;
    }

    let Some(ch) = cell.contents().chars().next() else {
        return;
    };

    draw_cell_glyph(image, origin_x, origin_y, ch, fg, glyphs, raster_scale);
    if cell.underline() {
        draw_underline(
            image,
            origin_x,
            origin_y,
            fg,
            raster_scale,
            cell_width,
            cell_height,
        );
    }
}

fn fill_cell_background(
    image: &mut RgbImage,
    origin_x: u32,
    origin_y: u32,
    color: [u8; 3],
    cell_width: u32,
    cell_height: u32,
) {
    for y in 0..cell_height {
        for x in 0..cell_width {
            image.put_pixel(origin_x + x, origin_y + y, Rgb(color));
        }
    }
}

fn draw_cell_glyph(
    image: &mut RgbImage,
    origin_x: u32,
    origin_y: u32,
    ch: char,
    color: [u8; 3],
    glyphs: &GlyphLookup,
    raster_scale: u32,
) {
    if ch == ' ' {
        return;
    }

    if ttf_antialias_enabled()
        && !prefers_bitmap_terminal_glyph(ch)
        && glyphs.draw_antialiased_glyph(image, origin_x, origin_y, ch, color, raster_scale)
    {
        return;
    }

    let glyph = glyphs
        .glyph(ch)
        .or_else(|| glyphs.glyph('?'))
        .unwrap_or([0_u8; 8]);

    for (glyph_row, row_bits) in glyph.into_iter().enumerate() {
        let glyph_row = u32::try_from(glyph_row).expect("glyph row in u32");
        for bit in 0_u8..8 {
            let pixel_is_on = row_bits & (1_u8 << bit) != 0;
            if !pixel_is_on {
                continue;
            }

            let pixel_x = origin_x + u32::from(bit) * raster_scale;
            let pixel_y = origin_y + glyph_row * GLYPH_VERTICAL_SCALE * raster_scale;
            for y in 0..(GLYPH_VERTICAL_SCALE * raster_scale) {
                for x in 0..raster_scale {
                    image.put_pixel(pixel_x + x, pixel_y + y, Rgb(color));
                }
            }
        }
    }
}

fn prefers_bitmap_terminal_glyph(ch: char) -> bool {
    let code = ch as u32;
    (0x2500..=0x259F).contains(&code)
}

fn draw_underline(
    image: &mut RgbImage,
    origin_x: u32,
    origin_y: u32,
    color: [u8; 3],
    raster_scale: u32,
    cell_width: u32,
    cell_height: u32,
) {
    let thickness = raster_scale.max(1);
    let y_start = origin_y + cell_height.saturating_sub(thickness);
    for y in 0..thickness {
        for x in 0..cell_width {
            image.put_pixel(origin_x + x, y_start + y, Rgb(color));
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
    anti_alias_cache: RefCell<BTreeMap<(char, u32), Arc<Vec<u8>>>>,
}

impl GlyphLookup {
    fn new() -> Self {
        Self {
            basic: BasicFonts::new(),
            latin: LatinFonts::new(),
            box_drawing: BoxFonts::new(),
            block: BlockFonts::new(),
            smooth_font: load_anti_alias_font(),
            anti_alias_cache: RefCell::new(BTreeMap::new()),
        }
    }

    fn draw_antialiased_glyph(
        &self,
        image: &mut RgbImage,
        origin_x: u32,
        origin_y: u32,
        ch: char,
        color: [u8; 3],
        raster_scale: u32,
    ) -> bool {
        let Some(mask) = self.anti_alias_mask_for(ch, raster_scale) else {
            return false;
        };

        let cell_width = GLYPH_WIDTH * raster_scale;
        let cell_height = GLYPH_HEIGHT * GLYPH_VERTICAL_SCALE * raster_scale;
        for y in 0..cell_height {
            for x in 0..cell_width {
                let idx = usize::try_from(y * cell_width + x).expect("mask index in usize");
                let alpha = mask[idx];
                if alpha == 0 {
                    continue;
                }

                let pixel = image.get_pixel_mut(origin_x + x, origin_y + y);
                let [r, g, b] = pixel.0;
                let a = u16::from(alpha);
                let inv = 255_u16.saturating_sub(a);
                pixel.0 = [
                    ((u16::from(r) * inv + u16::from(color[0]) * a + 127) / 255) as u8,
                    ((u16::from(g) * inv + u16::from(color[1]) * a + 127) / 255) as u8,
                    ((u16::from(b) * inv + u16::from(color[2]) * a + 127) / 255) as u8,
                ];
            }
        }

        true
    }

    fn anti_alias_mask_for(&self, ch: char, raster_scale: u32) -> Option<Arc<Vec<u8>>> {
        let cache_key = (ch, raster_scale);
        if let Some(mask) = self.anti_alias_cache.borrow().get(&cache_key) {
            return Some(mask.clone());
        }

        let font = self.smooth_font.as_ref()?;
        let cell_width = GLYPH_WIDTH * raster_scale;
        let cell_height = GLYPH_HEIGHT * GLYPH_VERTICAL_SCALE * raster_scale;
        let font_size = (cell_height as f32 * ANTI_ALIAS_FONT_SIZE_FACTOR).max(8.0);
        let line_metrics = font.horizontal_line_metrics(font_size);
        let reference_metrics = font.metrics('M', font_size);
        let baseline_origin_x =
            ((cell_width as f32 - reference_metrics.advance_width).max(0.0) * 0.5).round() as i32;
        let baseline_y = line_metrics
            .map(|line| {
                let centered_top =
                    ((cell_height as f32 - line.new_line_size).max(0.0) * 0.5).round();
                (centered_top + line.ascent).round() as i32
            })
            .unwrap_or_else(|| {
                i32::try_from(cell_height / 2).expect("cell height midpoint in i32")
            });

        let (metrics, bitmap) = font.rasterize(ch, font_size);
        let mut mask = vec![0_u8; (cell_width * cell_height) as usize];
        if metrics.width == 0 || metrics.height == 0 {
            let mask = Arc::new(mask);
            self.anti_alias_cache
                .borrow_mut()
                .insert(cache_key, mask.clone());
            return Some(mask);
        }

        let glyph_width = u32::try_from(metrics.width).expect("glyph width fits u32");
        let glyph_height = u32::try_from(metrics.height).expect("glyph height fits u32");
        let x_offset = baseline_origin_x + metrics.xmin;
        let y_offset =
            baseline_y - metrics.ymin - i32::try_from(glyph_height).expect("glyph height in i32");

        for y in 0..glyph_height {
            for x in 0..glyph_width {
                let src_idx = usize::try_from(y * glyph_width + x).expect("bitmap index in usize");
                let alpha = bitmap[src_idx];
                if alpha == 0 {
                    continue;
                }

                let dx = x_offset + i32::try_from(x).expect("x fits i32");
                let dy = y_offset + i32::try_from(y).expect("y fits i32");
                if dx < 0 || dy < 0 {
                    continue;
                }

                let dx = u32::try_from(dx).expect("dx non-negative");
                let dy = u32::try_from(dy).expect("dy non-negative");
                if dx >= cell_width || dy >= cell_height {
                    continue;
                }

                let dst_idx = usize::try_from(dy * cell_width + dx).expect("mask index in usize");
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

    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("HARNESS_VISUAL_FONT_PATH") {
        candidates.push(path);
    }
    candidates.push("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf".to_string());
    candidates.push("/usr/share/fonts/dejavu/DejaVuSansMono.ttf".to_string());

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

fn ttf_antialias_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("HARNESS_VISUAL_TTF_ANTIALIAS")
            .map(|value| {
                let normalized = value.trim();
                !(normalized == "0"
                    || normalized.eq_ignore_ascii_case("false")
                    || normalized.eq_ignore_ascii_case("off"))
            })
            .unwrap_or(true)
    })
}

fn prune_old_live_visual_runs(test_root: &Path, current_run_id: &str) -> Result<(), String> {
    let keep = std::env::var("HARNESS_LIVE_VISUAL_KEEP_RUNS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_LIVE_VISUAL_RETENTION_RUNS)
        .max(1);

    let mut runs = fs::read_dir(test_root)
        .map_err(|err| {
            format!(
                "failed to read live visual test root {}: {err}",
                test_root.display()
            )
        })?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let name = path.file_name()?.to_str()?.to_string();
            let modified = entry
                .metadata()
                .ok()?
                .modified()
                .ok()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some((modified, name, path))
        })
        .collect::<Vec<_>>();
    runs.sort_by_key(|(modified, name, _)| (std::cmp::Reverse(*modified), name.clone()));

    let mut kept = 0usize;
    for (_, name, path) in runs {
        if name == current_run_id || kept < keep {
            kept += 1;
            continue;
        }
        fs::remove_dir_all(&path).map_err(|err| {
            format!(
                "failed to remove stale live visual run {}: {err}",
                path.display()
            )
        })?;
    }

    Ok(())
}

pub fn default_live_run_metadata(
    provider: &str,
    model: &str,
    profile: &str,
    workspace_root: &Path,
    session_dir: &Path,
) -> Value {
    let viewport = selected_live_viewport();
    let timestamp = OffsetDateTime::now_utc()
        .format(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second]Z"
        ))
        .unwrap_or_else(|_| "unknown".to_string());
    json!({
        "created_at_utc": timestamp,
        "provider": provider,
        "model": model,
        "profile": profile,
        "workspace_root": workspace_root.display().to_string(),
        "session_dir": session_dir.display().to_string(),
        "viewport": {
            "preset": viewport.name,
            "rows": viewport.rows,
            "cols": viewport.cols,
            "cell_width": viewport.cell_width,
            "cell_height": viewport.cell_height,
        }
    })
}

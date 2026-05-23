use image::RgbImage;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use time::{macros::format_description, OffsetDateTime};
use vt100::Parser as VtParser;

use super::focus_region::anchored_region as anchored_focus_region;
use super::repo_root::repo_root;
use super::visual_manifest::{
    marker_presence_states, VISUAL_MANIFEST_JSONL_FILE, VISUAL_MANIFEST_JSON_FILE,
};
use super::visual_renderer::{
    extract_region_pixels, extract_region_render_state, render_parser_to_image,
    TerminalRenderConfig,
};

pub const CHECKPOINT_STARTUP: &str = "startup";
pub const CHECKPOINT_PERMISSION_REQUESTED: &str = "permission_requested";
pub const CHECKPOINT_PROMPT_STREAM: &str = "prompt_stream";
pub const CHECKPOINT_DRAFT_VISIBLE: &str = "draft_visible";
// Kept stable for existing visual artifact manifests; the stage now captures file creation
// through the high-level hashline-backed `edit` tool rather than a separate write tool.
pub const CHECKPOINT_FILE_WRITE_FINISHED: &str = "file_write_finished";
// Kept stable for existing visual artifact manifests; the stage now captures the high-level
// hashline-backed `edit` tool rather than the removed agent-facing scan tool.
pub const CHECKPOINT_HASHLINE_SCAN_FINISHED: &str = "hashline_scan_finished";
pub const CHECKPOINT_RUN_FINISHED: &str = "run_finished";
const LIVE_PROXY_NAMESPACE: &str = "live-proxy";
const LIVE_PROXY_PNG_PREFIX: &str = "live_proxy";
const DEFAULT_LIVE_VISUAL_RETENTION_RUNS: usize = 5;
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

pub(crate) fn parser_with_screen(lines: &[&str]) -> VtParser {
    let mut parser = VtParser::new(24, 80, 0);
    let mut frame = String::from("\u{1b}[2J\u{1b}[H");
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            frame.push('\n');
        }
        frame.push_str(line);
    }
    parser.process(frame.as_bytes());
    parser
}

pub(crate) fn write_tiny_png(path: &Path) {
    RgbImage::from_pixel(1, 1, image::Rgb([0, 0, 0]))
        .save(path)
        .unwrap_or_else(|err| panic!("failed to write tiny PNG {}: {err}", path.display()));
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
    pub artifact_namespace: String,
    pub png_prefix: String,
}

impl Default for LiveVisualRunOptions {
    fn default() -> Self {
        Self {
            run_metadata: Value::Object(serde_json::Map::new()),
            artifact_namespace: LIVE_PROXY_NAMESPACE.to_string(),
            png_prefix: LIVE_PROXY_PNG_PREFIX.to_string(),
        }
    }
}

pub struct ExternalPngCheckpointSpec<'a> {
    pub checkpoint_id: &'a str,
    pub source_png_path: &'a Path,
    pub screen_text: &'a str,
    pub terminal_size: (u16, u16),
    pub screen_markers: &'a [&'a str],
    pub focus: &'a FocusCapture,
    pub metadata: Option<&'a Value>,
}

#[derive(Debug)]
pub struct LiveVisualRun {
    run_dir: PathBuf,
    manifest: VisualManifest,
    png_prefix: String,
}

impl LiveVisualRun {
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
        validate_live_visual_identity(test_name, run_id)?;

        let test_root = root.join(&options.artifact_namespace).join(test_name);
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
            png_prefix: options.png_prefix,
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

        let render_config = live_visual_render_config();
        let image = render_parser_to_image(parser, render_config);
        let file_name = format!("{}_{checkpoint_id}.png", self.png_prefix);
        let png_path = self.run_dir.join(&file_name);
        image
            .save(&png_path)
            .map_err(|err| format!("failed to save {}: {err}", png_path.display()))?;

        let (rows, cols) = parser.screen().size();
        let focus_region = find_marker_cell(parser.screen(), &focus.marker)
            .map(|(row, col)| anchored_region((row, col), (rows, cols), focus));
        let focus_marker_found = focus_region.is_some();
        let focus_region_cells = focus_region.unwrap_or((0, 0, rows.max(1), cols.max(1)));
        let focus_pixels = extract_region_pixels(&image, focus_region_cells, render_config);
        let marker_states =
            marker_presence_states(parser.screen().contents().as_str(), screen_markers);
        let focus_scope = if focus_marker_found {
            "anchored"
        } else {
            "full_frame_fallback"
        };
        let focus_pixels_blake3 = blake3::hash(&focus_pixels).to_hex().to_string();
        let focus_render_state_blake3 = blake3::hash(&extract_region_render_state(
            parser.screen(),
            focus_region_cells,
        ))
        .to_hex()
        .to_string();
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
            focus_render_state_blake3: &focus_render_state_blake3,
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

    #[allow(dead_code)]
    pub fn capture_external_png_checkpoint(
        &mut self,
        spec: ExternalPngCheckpointSpec<'_>,
    ) -> Result<LiveVisualCheckpoint, String> {
        ensure_known_checkpoint_id(spec.checkpoint_id)?;

        let file_name = format!(
            "{}_{checkpoint_id}.png",
            self.png_prefix,
            checkpoint_id = spec.checkpoint_id
        );
        let png_path = self.run_dir.join(&file_name);
        if spec.source_png_path != png_path {
            fs::copy(spec.source_png_path, &png_path).map_err(|err| {
                format!(
                    "failed to copy external screenshot {} to {}: {err}",
                    spec.source_png_path.display(),
                    png_path.display()
                )
            })?;
        }

        let image = image::open(&png_path)
            .map_err(|err| {
                format!(
                    "failed to read external screenshot {}: {err}",
                    png_path.display()
                )
            })?
            .to_rgb8();
        let (rows, cols) = spec.terminal_size;
        if rows == 0 || cols == 0 {
            return Err(
                "external screenshot checkpoint requires non-zero terminal rows/cols".to_string(),
            );
        }

        let focus_region =
            find_marker_cell_in_text_grid(spec.screen_text, rows, cols, &spec.focus.marker)
                .map(|(row, col)| anchored_region((row, col), (rows, cols), spec.focus));
        let focus_marker_found = focus_region.is_some();
        let focus_region_cells = focus_region.unwrap_or((0, 0, rows.max(1), cols.max(1)));
        let render_config = external_png_render_config(&image, spec.terminal_size)?;
        let focus_pixels = extract_region_pixels(&image, focus_region_cells, render_config);
        let marker_states = marker_presence_states(spec.screen_text, spec.screen_markers);
        let focus_scope = if focus_marker_found {
            "anchored"
        } else {
            "full_frame_fallback"
        };
        let focus_pixels_blake3 = blake3::hash(&focus_pixels).to_hex().to_string();
        let focus_render_state_blake3 = blake3::hash(&extract_text_region_render_state(
            spec.screen_text,
            focus_region_cells,
            spec.terminal_size,
        ))
        .to_hex()
        .to_string();
        let manifest_entry = VisualManifestEntry::new(VisualManifestEntrySpec {
            checkpoint_id: spec.checkpoint_id,
            captured_at_stage: spec.checkpoint_id,
            png_path: &png_path,
            file_name: &file_name,
            screen_markers: &marker_states,
            focus_marker: &spec.focus.marker,
            focus_marker_found,
            focus_scope,
            focus_pixels_blake3: &focus_pixels_blake3,
            focus_render_state_blake3: &focus_render_state_blake3,
            focus_region_cells,
            image_size: (image.width(), image.height()),
            metadata: spec.metadata,
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
    pub focus_render_state_blake3: &'a str,
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
                    "render_state_blake3": spec.focus_render_state_blake3,
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
    pub fn new_in_with_metadata(
        output_dir: PathBuf,
        test_name: &str,
        run_id: &str,
        run_metadata: Value,
    ) -> Result<Self, String> {
        validate_live_visual_identity(test_name, run_id)?;

        fs::create_dir_all(&output_dir).map_err(|err| {
            format!(
                "failed to create live visual dir {}: {err}",
                output_dir.display()
            )
        })?;

        let manifest_json_path = output_dir.join(VISUAL_MANIFEST_JSON_FILE);
        let manifest_jsonl_path = output_dir.join(VISUAL_MANIFEST_JSONL_FILE);
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

fn validate_live_visual_identity(test_name: &str, run_id: &str) -> Result<(), String> {
    if test_name.trim().is_empty() {
        return Err("live visual test name cannot be empty".to_string());
    }
    if run_id.trim().is_empty() {
        return Err("live visual run id cannot be empty".to_string());
    }
    Ok(())
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
        | CHECKPOINT_FILE_WRITE_FINISHED
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
        CHECKPOINT_FILE_WRITE_FINISHED => 3,
        CHECKPOINT_HASHLINE_SCAN_FINISHED => 4,
        CHECKPOINT_PROMPT_STREAM => 5,
        CHECKPOINT_RUN_FINISHED => 6,
        _ => usize::MAX,
    }
}

#[allow(dead_code)]
fn find_marker_cell_in_text_grid(
    screen_text: &str,
    rows: u16,
    cols: u16,
    marker: &str,
) -> Option<(u16, u16)> {
    let lines = normalized_screen_lines(screen_text, rows, cols);
    for (row, line) in lines.iter().enumerate() {
        if let Some(byte_idx) = line.find(marker) {
            let col = line[..byte_idx].chars().count();
            if let (Ok(row), Ok(col)) = (u16::try_from(row), u16::try_from(col)) {
                return Some((row, col));
            }
        }
    }
    None
}

#[allow(dead_code)]
fn normalized_screen_lines(screen_text: &str, rows: u16, cols: u16) -> Vec<String> {
    let mut lines = screen_text
        .lines()
        .map(|line| {
            let mut normalized = line.chars().take(cols as usize).collect::<String>();
            let width = normalized.chars().count();
            if width < cols as usize {
                normalized.push_str(&" ".repeat(cols as usize - width));
            }
            normalized
        })
        .collect::<Vec<_>>();

    if lines.len() < rows as usize {
        lines.resize(rows as usize, " ".repeat(cols as usize));
    }
    if lines.len() > rows as usize {
        lines.truncate(rows as usize);
    }
    lines
}

#[allow(dead_code)]
fn extract_text_region_render_state(
    screen_text: &str,
    region: (u16, u16, u16, u16),
    terminal_size: (u16, u16),
) -> Vec<u8> {
    let (rows, cols) = terminal_size;
    let lines = normalized_screen_lines(screen_text, rows, cols);
    let (row_start, col_start, height_cells, width_cells) = region;
    let row_end = row_start.saturating_add(height_cells).min(rows);
    let col_end = col_start.saturating_add(width_cells).min(cols);
    let mut data = Vec::new();

    for row in row_start..row_end {
        let Some(line) = lines.get(row as usize) else {
            break;
        };
        let region_text = line
            .chars()
            .skip(col_start as usize)
            .take((col_end - col_start) as usize)
            .collect::<String>();
        data.extend_from_slice(region_text.as_bytes());
        data.push(b'\n');
    }

    data
}

#[allow(dead_code)]
fn external_png_render_config(
    image: &RgbImage,
    terminal_size: (u16, u16),
) -> Result<TerminalRenderConfig, String> {
    let (rows, cols) = terminal_size;
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return Err("external screenshot image dimensions must be non-zero".to_string());
    }

    let cols = u32::from(cols);
    let rows = u32::from(rows);
    if !width.is_multiple_of(cols) || !height.is_multiple_of(rows) {
        return Err(format!(
            "external screenshot {}x{} does not map cleanly to {}x{} terminal cells",
            width, height, cols, rows
        ));
    }

    Ok(TerminalRenderConfig::new(width / cols, height / rows))
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
        return resolve_artifact_root(dir);
    }

    repo_root().join("target").join("pty-visual-artifacts")
}

fn resolve_artifact_root(dir: impl Into<PathBuf>) -> PathBuf {
    let dir = dir.into();
    if dir.is_absolute() {
        dir
    } else {
        repo_root().join(dir)
    }
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
    anchor: (u16, u16),
    bounds: (u16, u16),
    focus: &FocusCapture,
) -> (u16, u16, u16, u16) {
    anchored_focus_region(
        anchor,
        bounds,
        focus.width_cells,
        focus.height_cells,
        focus.top_padding_cells,
        focus.left_padding_cells,
    )
}

fn live_visual_render_config() -> TerminalRenderConfig {
    let viewport = selected_live_viewport();
    TerminalRenderConfig::new(viewport.cell_width, viewport.cell_height)
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
            let is_manifest_backed_run = path.join(VISUAL_MANIFEST_JSON_FILE).exists()
                || path.join(VISUAL_MANIFEST_JSONL_FILE).exists();
            if name != current_run_id && !is_manifest_backed_run {
                return None;
            }
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
    variant: Option<&str>,
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
        "variant": variant,
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

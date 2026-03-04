use font8x8::unicode::{BasicFonts, BlockFonts, BoxFonts, LatinFonts, UnicodeFonts};
use fontdue::{Font, FontSettings};
use image::{imageops::FilterType, Rgb, RgbImage};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::cell::RefCell;
use std::cmp;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    mpsc::{self, Receiver, RecvTimeoutError},
    Arc, OnceLock,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vt100::{Color as VtColor, Parser as VtParser};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PTY_COLS: u16 = 80;
const PTY_ROWS: u16 = 24;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const MARKER_TIMEOUT: Duration = Duration::from_secs(6);
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);
const STABLE_WINDOW: Duration = Duration::from_millis(180);
const STABLE_TIMEOUT: Duration = Duration::from_secs(2);
const GLYPH_WIDTH: u32 = 8;
const GLYPH_HEIGHT: u32 = 8;
const GLYPH_VERTICAL_SCALE: u32 = 2;
const RASTER_SCALE: u32 = 4;
const RASTER_CELL_WIDTH: u32 = GLYPH_WIDTH * RASTER_SCALE;
const RASTER_CELL_HEIGHT: u32 = GLYPH_HEIGHT * GLYPH_VERTICAL_SCALE * RASTER_SCALE;
const CELL_WIDTH: u32 = 32;
const CELL_HEIGHT: u32 = 60;
const DEFAULT_FG: [u8; 3] = [216, 216, 216];
const DEFAULT_BG: [u8; 3] = [18, 18, 18];
const ANTI_ALIAS_FONT_SIZE_FACTOR: f32 = 0.72;

#[tokio::test(flavor = "current_thread")]
async fn pty_e2e_tui_golden_path() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let harness_bin = resolve_harness_bin();
    let repo_root = repo_root();
    let session_dir = create_temp_session_dir();

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: PTY_ROWS,
            cols: PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pty pair");

    let mut command = CommandBuilder::new(harness_bin.to_string_lossy().as_ref());
    command.arg("tui");
    command.arg("--scenario");
    command.arg("golden_path_interactive");
    command.arg("--deterministic");
    command.arg("--session-dir");
    command.arg(session_dir.to_string_lossy().to_string());
    command.cwd(repo_root);
    command.env("HARNESS_DETERMINISTIC", "1");
    command.env("HARNESS_DISABLE_ANIMATIONS", "1");
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    command.env("LC_ALL", "C.UTF-8");
    command.env("TZ", "UTC");

    let child = pair
        .slave
        .spawn_command(command)
        .expect("spawn harness tui command");
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().expect("clone pty reader");
    let mut writer = pair.master.take_writer().expect("take pty writer");
    let output_rx = spawn_reader_thread(reader);
    let mut parser = VtParser::new(PTY_ROWS, PTY_COLS, 0);
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    wait_for_screen_contains(&mut parser, &output_rx, "Prompt", STARTUP_TIMEOUT)
        .expect("wait for initial TUI render");

    send_key(writer.as_mut(), b' ').expect("disable follow mode for deterministic captures");
    for _ in 0..64 {
        send_key(writer.as_mut(), b'k').expect("move selection to first event");
    }

    let permission_checkpoint = wait_for_screen_contains(
        &mut parser,
        &output_rx,
        "Permission Requested",
        MARKER_TIMEOUT,
    )
    .expect("wait for permission marker");
    let permission_visual = capture_visual_checkpoint(
        "permission_requested",
        &parser,
        &visual_dir,
        FocusCapture::anchored_exact("Permission Requested", 24, 1),
    )
    .expect("capture permission checkpoint image");
    insta::assert_snapshot!(
        "pty_permission_requested",
        checkpoint_visual_snapshot(
            &permission_checkpoint,
            &["Permission Requested"],
            &permission_visual
        )
    );

    send_key(writer.as_mut(), b'a').expect("send approve key");
    send_key(writer.as_mut(), b' ').expect("re-enable follow mode after permission capture");

    let run_finished_checkpoint =
        wait_for_screen_contains(&mut parser, &output_rx, "24 events", MARKER_TIMEOUT)
            .expect("wait for run finished marker");
    let run_finished_visual = capture_visual_checkpoint(
        "run_finished",
        &parser,
        &visual_dir,
        FocusCapture::anchored_exact("24 events", 12, 1),
    )
    .expect("capture run finished checkpoint image");
    insta::assert_snapshot!(
        "pty_run_finished",
        checkpoint_visual_snapshot(
            &run_finished_checkpoint,
            &["worker-prompt-delta", "Status: done", "24 events"],
            &run_finished_visual
        )
    );

    send_key(writer.as_mut(), b'3').expect("switch to diff tab");
    let diff_checkpoint = wait_for_screen_contains(
        &mut parser,
        &output_rx,
        "diff artifact missing:",
        MARKER_TIMEOUT,
    )
    .expect("wait for diff contents marker");
    let diff_visual = capture_visual_checkpoint(
        "diff_tab",
        &parser,
        &visual_dir,
        FocusCapture::anchored_exact("diff artifact missing:", 24, 1),
    )
    .expect("capture diff image");
    insta::assert_snapshot!(
        "pty_diff_tab",
        checkpoint_visual_snapshot(
            &diff_checkpoint,
            &["diff artifact missing:", "Tabs", "24 events"],
            &diff_visual
        )
    );

    send_key(writer.as_mut(), b'q').expect("send quit key");
    drop(writer);

    let status = wait_for_child_exit(child, EXIT_TIMEOUT);
    assert!(
        status.success(),
        "expected harness tui to exit with status 0, got {status:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn pty_e2e_tui_interactive_prompt_streams_response() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let server = MockServer::start().await;
    let sse_body = responses_api_sse_fixture();
    let response_template = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(sse_body.clone(), "text/event-stream");

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(response_template.clone())
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(response_template)
        .mount(&server)
        .await;

    let harness_bin = resolve_harness_bin();
    let repo_root = repo_root();
    let session_dir = create_temp_session_dir();
    let config_path = write_wiremock_tui_config(&session_dir, &server.uri());

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: PTY_ROWS,
            cols: PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pty pair");

    let mut command = CommandBuilder::new(harness_bin.to_string_lossy().as_ref());
    command.arg("tui");
    command.arg("--config");
    command.arg(config_path.to_string_lossy().to_string());
    command.arg("--deterministic");
    command.arg("--session-dir");
    command.arg(session_dir.to_string_lossy().to_string());
    command.cwd(repo_root);
    command.env("HARNESS_DETERMINISTIC", "1");
    command.env("HARNESS_DISABLE_ANIMATIONS", "1");
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    command.env("LC_ALL", "C.UTF-8");
    command.env("TZ", "UTC");

    let child = pair
        .slave
        .spawn_command(command)
        .expect("spawn harness tui command");
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().expect("clone pty reader");
    let mut writer = pair.master.take_writer().expect("take pty writer");
    let output_rx = spawn_reader_thread(reader);
    let mut parser = VtParser::new(PTY_ROWS, PTY_COLS, 0);
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    wait_for_screen_contains(&mut parser, &output_rx, "Prompt", STARTUP_TIMEOUT)
        .expect("wait for initial TUI render");

    send_key(writer.as_mut(), b'\t').expect("focus details pane");
    send_key(writer.as_mut(), b'\t').expect("focus prompt pane");

    writer
        .write_all(b"Hello from PTY")
        .expect("type prompt text");
    writer.flush().expect("flush prompt text");
    send_key(writer.as_mut(), b'\r').expect("submit prompt");

    let prompt_checkpoint =
        wait_for_screen_contains(&mut parser, &output_rx, "Hello world", MARKER_TIMEOUT)
            .expect("wait for streamed response text marker");
    let prompt_visual = capture_visual_checkpoint(
        "interactive_prompt_stream",
        &parser,
        &visual_dir,
        FocusCapture::anchored("Hello world", 28, 6),
    )
    .expect("capture interactive prompt checkpoint image");
    insta::assert_snapshot!(
        "pty_interactive_prompt_stream",
        checkpoint_visual_snapshot(
            &prompt_checkpoint,
            &["Hello world", "Prompt"],
            &prompt_visual
        )
    );

    drop(writer);

    let mut child = child;
    child.kill().expect("terminate interactive tui child");
    std::mem::forget(child);
}

fn responses_api_sse_fixture() -> String {
    concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_123\"}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}

fn write_wiremock_tui_config(session_dir: &Path, wiremock_uri: &str) -> PathBuf {
    let config_path = session_dir.join("wiremock-tui-config.jsonc");
    let body = format!(
        r#"{{
  backgroundTask: {{
    defaultConcurrency: 2,
    providerConcurrency: 2,
    modelConcurrency: 2,
    staleTimeoutMs: 15000,
    messageStalenessTimeoutMs: 5000,
  }},
  providers: {{
    default: {{
      type: "openai_compatible",
      base_url: "{wiremock_uri}/v1",
      api_key: "test-key",
      api_mode: "responses",
      models: {{
        "model-1": {{
          display_name: "Model 1",
        }},
      }},
    }},
  }},
  categories: {{
    deep: {{
      description: "deep",
      model_ref: "default:model-1",
      tools: ["read"],
    }},
  }},
  permissions: {{
    edit: "ask",
    shell: "deny",
    network: "deny",
  }},
  paths: {{
    session_dir: "{}",
  }},
  deterministic: {{
    enabled: true,
    seed: 42,
  }},
  ui: {{
    default_profile: "worker",
  }},
}}"#,
        session_dir.display()
    );
    fs::write(&config_path, body).expect("write temporary wiremock TUI config");
    config_path
}

fn wait_for_screen_contains(
    parser: &mut vt100::Parser,
    output_rx: &Receiver<Vec<u8>>,
    needle: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_output(parser, output_rx);

        let current = screen_contents(parser);
        if current.contains(needle) {
            return Ok(stabilize_screen(parser, output_rx, current));
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for screen marker '{needle}' after {timeout:?}; final screen:\n{current}"
            ));
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "pty output stream closed while waiting for '{needle}'; last screen:\n{current}"
                ));
            }
        }
    }
}

fn stabilize_screen(
    parser: &mut vt100::Parser,
    output_rx: &Receiver<Vec<u8>>,
    initial: String,
) -> String {
    let mut latest = initial;
    let mut stable_since = Instant::now();
    let deadline = Instant::now() + STABLE_TIMEOUT;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return latest;
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return latest,
        }

        let current = screen_contents(parser);
        if current != latest {
            latest = current;
            stable_since = Instant::now();
            continue;
        }

        if Instant::now().saturating_duration_since(stable_since) >= STABLE_WINDOW {
            return latest;
        }
    }
}

fn drain_output(parser: &mut vt100::Parser, output_rx: &Receiver<Vec<u8>>) {
    while let Ok(chunk) = output_rx.try_recv() {
        parser.process(&chunk);
    }
}

fn screen_contents(parser: &vt100::Parser) -> String {
    parser.screen().contents()
}

#[derive(Debug)]
struct VisualCheckpoint {
    file_name: String,
    focus_pixels_blake3: String,
    focus_marker: String,
    focus_region_cells: (u16, u16, u16, u16),
    size_px: (u32, u32),
}

#[derive(Debug, Clone, Copy)]
struct FocusCapture {
    marker: &'static str,
    width_cells: u16,
    height_cells: u16,
    top_padding_cells: u16,
    left_padding_cells: u16,
}

impl FocusCapture {
    fn anchored(marker: &'static str, width_cells: u16, height_cells: u16) -> Self {
        Self {
            marker,
            width_cells,
            height_cells,
            top_padding_cells: 2,
            left_padding_cells: 2,
        }
    }

    fn anchored_exact(marker: &'static str, width_cells: u16, height_cells: u16) -> Self {
        Self {
            marker,
            width_cells,
            height_cells,
            top_padding_cells: 0,
            left_padding_cells: 0,
        }
    }
}

fn checkpoint_visual_snapshot(screen: &str, markers: &[&str], visual: &VisualCheckpoint) -> String {
    let mut lines = marker_presence_lines(screen, markers);
    lines.push(format!("focus_marker: {}", visual.focus_marker));
    lines.push(format!(
        "focus_pixels_blake3: {}",
        visual.focus_pixels_blake3
    ));
    lines.push(format!(
        "focus_region_cells: row={}, col={}, height={}, width={}",
        visual.focus_region_cells.0,
        visual.focus_region_cells.1,
        visual.focus_region_cells.2,
        visual.focus_region_cells.3
    ));
    lines.push(format!("image: {}", visual.file_name));
    lines.push(format!(
        "size_px: {}x{}",
        visual.size_px.0, visual.size_px.1
    ));
    lines.join("\n")
}

fn marker_presence_lines(screen: &str, markers: &[&str]) -> Vec<String> {
    markers
        .iter()
        .map(|marker| format!("{marker}: {}", screen.contains(marker)))
        .collect()
}

fn capture_visual_checkpoint(
    name: &str,
    parser: &VtParser,
    visual_dir: &Path,
    focus: FocusCapture,
) -> Result<VisualCheckpoint, String> {
    let image = render_parser_to_image(parser);
    let file_name = format!("pty_{name}.png");
    let path = visual_dir.join(&file_name);
    image
        .save(&path)
        .map_err(|err| format!("failed to save {}: {err}", path.display()))?;

    let (rows, cols) = parser.screen().size();
    let focus_region = find_marker_cell(parser.screen(), focus.marker)
        .map(|(row, col)| anchored_region(row, col, rows, cols, focus))
        .unwrap_or((0, 0, rows.max(1), cols.max(1)));

    let focus_pixels = extract_region_pixels(&image, focus_region);

    Ok(VisualCheckpoint {
        file_name,
        focus_pixels_blake3: blake3::hash(&focus_pixels).to_hex().to_string(),
        focus_marker: focus.marker.to_string(),
        focus_region_cells: focus_region,
        size_px: (image.width(), image.height()),
    })
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
    focus: FocusCapture,
) -> (u16, u16, u16, u16) {
    let row_start = anchor_row.saturating_sub(focus.top_padding_cells);
    let col_start = anchor_col.saturating_sub(focus.left_padding_cells);

    let max_height = rows.saturating_sub(row_start).max(1);
    let max_width = cols.saturating_sub(col_start).max(1);

    let height = focus.height_cells.min(max_height).max(1);
    let width = focus.width_cells.min(max_width).max(1);

    (row_start, col_start, height, width)
}

fn extract_region_pixels(image: &RgbImage, region: (u16, u16, u16, u16)) -> Vec<u8> {
    let (row_start, col_start, height_cells, width_cells) = region;

    let x_start = u32::from(col_start) * CELL_WIDTH;
    let y_start = u32::from(row_start) * CELL_HEIGHT;
    let width_px = u32::from(width_cells) * CELL_WIDTH;
    let height_px = u32::from(height_cells) * CELL_HEIGHT;

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

    let target_width = u32::from(cols) * CELL_WIDTH;
    let target_height = u32::from(rows) * CELL_HEIGHT;
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
    anti_alias_cache: AntiAliasCache,
}

type AntiAliasCacheKey = (char, u32);
type AntiAliasMask = Arc<Vec<u8>>;
type AntiAliasCache = RefCell<BTreeMap<AntiAliasCacheKey, AntiAliasMask>>;

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

fn visual_artifacts_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("HARNESS_VISUAL_ARTIFACT_DIR") {
        return PathBuf::from(dir);
    }

    repo_root().join("target").join("pty-visual-artifacts")
}

fn send_key(writer: &mut dyn Write, key: u8) -> std::io::Result<()> {
    writer.write_all(&[key])?;
    writer.flush()
}

fn wait_for_child_exit(
    mut child: Box<dyn portable_pty::Child + Send>,
    timeout: Duration,
) -> portable_pty::ExitStatus {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = child.wait();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => panic!("wait for harness process failed: {err}"),
        Err(RecvTimeoutError::Timeout) => {
            panic!("timed out waiting {timeout:?} for harness process to exit")
        }
        Err(RecvTimeoutError::Disconnected) => {
            panic!("harness process wait channel disconnected before receiving status")
        }
    }
}

fn spawn_reader_thread(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if tx.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    rx
}

fn resolve_harness_bin() -> PathBuf {
    if let Ok(path) = std::env::var("HARNESS_BIN") {
        let harness_bin = PathBuf::from(path);
        assert!(
            harness_bin.exists(),
            "HARNESS_BIN points to missing path: {}",
            harness_bin.display()
        );
        return harness_bin;
    }

    let repo = repo_root();
    let status = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("harness")
        .current_dir(&repo)
        .status()
        .expect("spawn cargo build -p harness");
    assert!(
        status.success(),
        "cargo build -p harness failed with status {status}"
    );

    let harness_bin = repo
        .join("target")
        .join("debug")
        .join(binary_name("harness"));
    assert!(
        harness_bin.exists(),
        "expected harness binary at {}",
        harness_bin.display()
    );
    harness_bin
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("harness-testkit should live under <repo>/crates/harness-testkit")
}

fn create_temp_session_dir() -> PathBuf {
    let base = std::env::temp_dir().join("harness-testkit");
    fs::create_dir_all(&base).expect("create base temp session dir");

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    let dir = base.join(format!("pty-e2e-{}-{suffix}", std::process::id()));
    fs::create_dir_all(&dir).expect("create unique temp session dir");
    dir
}

#[cfg(target_os = "windows")]
fn binary_name(name: &str) -> String {
    format!("{name}.exe")
}

#[cfg(not(target_os = "windows"))]
fn binary_name(name: &str) -> String {
    name.to_string()
}

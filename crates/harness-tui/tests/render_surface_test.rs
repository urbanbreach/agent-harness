//! Deterministic render surface + frame-observation seam contract tests.
//!
//! Todo 11 (Wave 1) of the Harness clean-room parity program. This file
//! defines a clean-room, test-local render surface built on the existing
//! `harness_tui::render_test::render_to_buffer` primitive (Ratatui `TestBackend`)
//! plus `harness_tui::ui::render_app`. No external reference source is read,
//! copied, or transformed here — only behavior contracts are asserted.
//!
//! ## Proof dimensions covered
//! - P1 contract : frame capture is deterministic (repeat + independent builds).
//! - P2 owner    : the surface lives in `harness-tui/tests/` and drives the TUI's
//!   own `ui::render_app`, owned by this crate.
//! - P3 terminal : captured frame geometry/content matches the live shell layout
//!   plan across the shell-contract viewports.
//! - P4 raster   : per-cell foreground/background/modifier raster is captured and
//!   stable, enabling zero-tolerance raster comparison later.
//! - P6 rejection: the observation seam must NOT affect rendering — hooks observe
//!   through shared references only, and an observed frame is byte-
//!   identical to a directly captured frame.
//!
//! ## Differential TDD note
//! Two functions encode the seam's completeness and are the red/green switch:
//!   * `capture_cell_style` — naive returns a default (empty) style, so P4 raster
//!     contracts fail; complete returns the real per-cell style.
//!   * `observer_capture_area` — naive perturbs the capture geometry, so P6
//!     non-interference contracts fail; complete uses the exact viewport.
//!
//! Delivered state is COMPLETE (both behave correctly).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use harness_core::event::{
    ActorKind, EditAppliedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    RunStartedEvent, ToolCallFinishedEvent, ToolCallMetadata, ToolCallRequestedEvent,
    ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, LaunchMetadata};
use harness_tui::render_test::render_to_buffer;
use harness_tui::ui;
use harness_tui::FrameLayoutPlan;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

/// Fixed deterministic viewport (one of the shell-contract geometry targets).
const DET_WIDTH: u16 = 120;
const DET_HEIGHT: u16 = 40;

// ---------------------------------------------------------------------------
// SemanticFrame — a testable capture of one rendered frame.
// ---------------------------------------------------------------------------

/// Per-cell style fingerprint for raster comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellStyle {
    fg: Color,
    bg: Color,
    modifier_bits: u16,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            fg: Color::Reset,
            bg: Color::Reset,
            modifier_bits: Modifier::empty().bits(),
        }
    }
}

impl CellStyle {
    fn from_cell(cell: &Cell) -> Self {
        Self {
            fg: cell.fg,
            bg: cell.bg,
            modifier_bits: cell.modifier.bits(),
        }
    }

    fn is_default(self) -> bool {
        self.fg == Color::Reset
            && self.bg == Color::Reset
            && self.modifier_bits == Modifier::empty().bits()
    }

    /// Compact, deterministic encoding used for raster digests / snapshots.
    fn encode(self) -> String {
        format!("{:?}|{:?}|{}", self.fg, self.bg, self.modifier_bits)
    }
}

/// Deterministic capture of one rendered frame: text grid + per-cell style raster.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticFrame {
    width: u16,
    height: u16,
    /// Text grid, one entry per row (symbols concatenated, `buffer_to_string` shape).
    rows: Vec<String>,
    /// Style raster, indexed `[row][col]`.
    raster: Vec<Vec<CellStyle>>,
}

impl SemanticFrame {
    fn text(&self) -> String {
        self.rows.join("\n")
    }

    /// Count of cells carrying a non-default foreground/background/modifier.
    /// A styled shell must be non-zero; a naive text-only seam yields zero.
    fn non_default_style_count(&self) -> usize {
        self.raster
            .iter()
            .flatten()
            .filter(|style| !style.is_default())
            .count()
    }

    /// Deterministic encoding of the whole style raster for raster comparison.
    fn style_digest(&self) -> String {
        let mut out = String::new();
        for row in &self.raster {
            for cell in row {
                out.push_str(&cell.encode());
                out.push(';');
            }
            out.push('\n');
        }
        out
    }
}

/// Render the app into a Ratatui buffer for the given viewport using `render_app`.
fn render_buffer(app: &AppState, area: Rect) -> Buffer {
    render_to_buffer(app, area, |app, frame, _area| ui::render_app(frame, app))
}

/// Capture a [`SemanticFrame`] for the app at the given viewport.
///
/// Deterministic: the same `AppState` + `area` always yields an identical frame
/// (TestBackend has no wall clock, no terminal, no animation advance here).
fn capture_semantic_frame(app: &AppState, area: Rect) -> SemanticFrame {
    let buffer = render_buffer(app, area);
    let width = usize::from(area.width);
    let mut rows = Vec::with_capacity(usize::from(area.height));
    let mut raster = Vec::with_capacity(usize::from(area.height));
    for row in buffer.content.chunks(width) {
        let mut text = String::new();
        let mut styles = Vec::with_capacity(row.len());
        for cell in row {
            text.push_str(cell.symbol());
            styles.push(capture_cell_style(cell));
        }
        rows.push(text);
        raster.push(styles);
    }
    SemanticFrame {
        width: area.width,
        height: area.height,
        rows,
        raster,
    }
}

/// TDD red/green switch (P4). COMPLETE: capture the real per-cell foreground,
/// background, and modifier so the raster reflects the rendered shell.
fn capture_cell_style(cell: &Cell) -> CellStyle {
    CellStyle::from_cell(cell)
}

// ---------------------------------------------------------------------------
// FrameObserver — a passive frame-observation seam.
// ---------------------------------------------------------------------------

/// Passive observation seam. Hooks receive only shared references
/// (`&AppState` before render, `&SemanticFrame` after render), so they can
/// observe but never mutate state or the produced frame. Whether or not an
/// observer is attached, and however many hooks run, the captured frame must be
/// identical to a direct capture at the same viewport.
struct FrameObserver<'a> {
    before_render: Vec<Box<dyn Fn(&AppState) + 'a>>,
    after_render: Vec<Box<dyn Fn(&SemanticFrame) + 'a>>,
    frames: Vec<SemanticFrame>,
}

impl<'a> FrameObserver<'a> {
    fn new() -> Self {
        Self {
            before_render: Vec::new(),
            after_render: Vec::new(),
            frames: Vec::new(),
        }
    }

    fn on_before_render(mut self, hook: impl Fn(&AppState) + 'a) -> Self {
        self.before_render.push(Box::new(hook));
        self
    }

    fn on_after_render(mut self, hook: impl Fn(&SemanticFrame) + 'a) -> Self {
        self.after_render.push(Box::new(hook));
        self
    }

    /// Observe one render. Runs before-hooks over the (shared) state, captures a
    /// frame, then runs after-hooks over the (shared) frame. Records the frame.
    fn capture(&mut self, app: &AppState, area: Rect) -> SemanticFrame {
        for hook in &self.before_render {
            hook(app);
        }
        let frame = capture_semantic_frame(app, observer_capture_area(area));
        for hook in &self.after_render {
            hook(&frame);
        }
        self.frames.push(frame.clone());
        frame
    }

    fn frames(&self) -> &[SemanticFrame] {
        &self.frames
    }
}

/// TDD red/green switch (P6). COMPLETE: observe at the exact requested geometry,
/// so an attached observer never perturbs the captured frame.
fn observer_capture_area(area: Rect) -> Rect {
    area
}

// ---------------------------------------------------------------------------
// Fixtures (deterministic, replay-style event sequences).
// ---------------------------------------------------------------------------

fn det_envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-rs-{seq:04}"),
        seq,
        run_id: "run_render_surface".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("render-surface".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_render_surface".to_string()),
        payload,
    }
}

fn styled_session_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_render_surface";
    vec![
        det_envelope(
            1,
            Some(request_id),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "render-surface".into(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        det_envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "Inspect the deterministic render surface".to_string(),
            }),
        ),
        det_envelope(
            3,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Inspect the deterministic render surface".to_string(),
                request_digest: "digest-render-surface".to_string(),
                metadata: None,
            }),
        ),
        det_envelope(
            4,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "The shell rendered deterministically across captures.".to_string(),
            }),
        ),
        det_envelope(
            5,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_read".into(),
                tool_id: "read".to_string(),
                args_summary: r#"{"path":"src/ui.rs"}"#.to_string(),
                args_digest: "digest-tc-read".to_string(),
                metadata: Some(ToolCallMetadata {
                    canonical_tool_id: Some("read".to_string()),
                    ..Default::default()
                }),
            }),
        ),
        det_envelope(
            6,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_read".into(),
            }),
        ),
        det_envelope(
            7,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_read".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("Read the render module".to_string()),
                output_digest: Some("digest-tc-read-output".to_string()),
                output_json: None,
                metadata: Some(ToolCallMetadata {
                    canonical_tool_id: Some("read".to_string()),
                    ..Default::default()
                }),
            }),
        ),
        det_envelope(
            8,
            Some(request_id),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit_render_surface".to_string(),
                path: "src/ui.rs".to_string(),
                new_file_digest: "digest-edit-render-surface".to_string(),
                diff_rel_path: None,
                diff_digest: None,
            }),
        ),
        det_envelope(
            9,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-render-surface-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ]
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_render_surface")), false, None);
    for event in styled_session_events() {
        app.ingest_event(event);
    }
    app
}

fn startup_app() -> AppState {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );
    app.composer.prompt_buffer = "Deterministic render surface draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.len();
    app
}

fn viewport() -> Rect {
    Rect::new(0, 0, DET_WIDTH, DET_HEIGHT)
}

// ---------------------------------------------------------------------------
// Contract tests
// ---------------------------------------------------------------------------

/// P1: capturing the same app state twice yields an identical SemanticFrame
/// (text and raster). The seam has no wall-clock/animation dependence.
#[test]
fn frame_capture_is_deterministic() {
    let app = live_app();
    let first = capture_semantic_frame(&app, viewport());
    let second = capture_semantic_frame(&app, viewport());
    assert_eq!(
        first, second,
        "repeat capture of identical state must be frame-identical"
    );
    assert_eq!(first.style_digest(), second.style_digest());
}

/// P1+P4 (naive: raster only) P4: a rendered shell carries styled cells, so the
/// raster must contain non-default entries. NAIVE seam (text-only) fails here.
#[test]
fn semantic_frame_captures_styled_cells() {
    let app = live_app();
    let frame = capture_semantic_frame(&app, viewport());
    assert!(
        !frame.text().trim().is_empty(),
        "captured frame must contain rendered text"
    );
    assert!(
        frame.non_default_style_count() > 0,
        "style raster must capture the shell's styled cells (P4); got {}",
        frame.non_default_style_count()
    );
}

/// P1: two independently constructed apps with identical events produce identical
/// text and style rasters (input-level determinism, no shared-state bleed).
#[test]
fn input_level_determinism_across_independent_builds() {
    let a = capture_semantic_frame(&live_app(), viewport());
    let b = capture_semantic_frame(&live_app(), viewport());
    assert_eq!(a.text(), b.text(), "text must be input-deterministic");
    assert_eq!(
        a.style_digest(),
        b.style_digest(),
        "style raster must be input-deterministic"
    );
}

/// P6 control (anti-vacuity): genuinely different surfaces must produce
/// different frames, so equality assertions above are meaningful.
#[test]
fn distinct_surfaces_produce_distinct_frames() {
    let live = capture_semantic_frame(&live_app(), viewport());
    let startup = capture_semantic_frame(&startup_app(), viewport());
    assert_ne!(
        live.text(),
        startup.text(),
        "different surfaces must capture different frames"
    );
}

/// P6: an observed frame must be byte-identical to a direct capture at the same
/// viewport. NAIVE seam perturbs geometry, so this fails until the seam is
/// passive.
#[test]
fn observation_seam_does_not_affect_render() {
    let app = live_app();
    let mut observer = FrameObserver::new();
    let observed = observer.capture(&app, viewport());
    let direct = capture_semantic_frame(&app, viewport());
    assert_eq!(
        observed, direct,
        "attaching an observer must not change the captured frame (P6)"
    );
    assert_eq!(observer.frames().len(), 1);
}

/// P6/P1: hooks observe through shared references only. They record the frame
/// geometry they saw, and that geometry must equal the requested viewport; the
/// captured frame is unaffected by how many hooks run.
#[test]
fn observer_hooks_observe_without_side_effects() {
    let app = live_app();
    let seen_before = Rc::new(RefCell::new(0usize));
    let seen_dims = Rc::new(RefCell::new(Vec::<(u16, u16)>::new()));

    let before = Rc::clone(&seen_before);
    let dims = Rc::clone(&seen_dims);
    let mut observer = FrameObserver::new()
        .on_before_render(move |_app| {
            *before.borrow_mut() += 1;
        })
        .on_after_render(move |frame| {
            dims.borrow_mut().push((frame.width, frame.height));
        });

    let observed = observer.capture(&app, viewport());

    assert_eq!(*seen_before.borrow(), 1, "before-render hook ran once");
    assert_eq!(
        *seen_dims.borrow(),
        vec![(DET_WIDTH, DET_HEIGHT)],
        "after-render hook must see the requested viewport exactly"
    );
    assert_eq!(
        observed,
        capture_semantic_frame(&app, viewport()),
        "hooks must not perturb the produced frame"
    );
}

/// P3: the captured frame's geometry and content must agree with the live shell
/// layout plan — transcript present, composer dock present, ingested user text
/// and assistant delta appear in the rendered grid.
#[test]
fn frame_layout_matches_shell_contract() {
    let app = live_app();
    let area = viewport();
    let plan = FrameLayoutPlan::for_app(&app, area);
    assert!(
        plan.transcript.is_some(),
        "live shell keeps a transcript region"
    );
    assert!(plan.composer.is_some(), "live shell keeps a composer dock");

    let frame = capture_semantic_frame(&app, area);
    assert_eq!(frame.width, area.width, "frame width matches the viewport");
    assert_eq!(
        frame.height, area.height,
        "frame height matches the viewport"
    );

    let text = frame.text();
    assert!(
        text.contains("Inspect the deterministic render surface"),
        "ingested user message must appear in the captured transcript\n{text}"
    );
    assert!(
        text.contains("deterministically"),
        "assistant delta must appear in the captured transcript\n{text}"
    );
}

/// P3/P1/P4: every shell-contract viewport renders deterministically and carries
/// styled cells. NAIVE seam fails the raster assertion on each viewport.
#[test]
fn viewport_matrix_is_deterministic_and_styled() {
    for (width, height) in [(80u16, 24u16), (100, 30), (120, 40)] {
        let area = Rect::new(0, 0, width, height);
        let app = live_app();
        let first = capture_semantic_frame(&app, area);
        let second = capture_semantic_frame(&app, area);
        assert_eq!(
            first, second,
            "viewport {width}x{height} must be deterministic"
        );
        assert_eq!(
            (first.width, first.height),
            (width, height),
            "frame geometry must match the requested viewport {width}x{height}"
        );
        assert!(
            first.non_default_style_count() > 0,
            "viewport {width}x{height} must capture styled cells (P4); got {}",
            first.non_default_style_count()
        );
    }
}

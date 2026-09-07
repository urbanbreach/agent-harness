use std::{cell::RefCell, rc::Rc};

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};

use crate::{app::AppState, render_test::render_to_buffer, ui};

fn app(source: &str, settled: bool) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_reduced_motion_for_evidence(true);
    let mut payloads = vec![
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "grammar".into(),
            text: "QA".into(),
        }),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "grammar".into(),
            provider_id: "mock".into(),
            model_id: "grammar".into(),
            prompt_summary: "QA".into(),
            request_digest: "grammar".into(),
            metadata: None,
        }),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "grammar".into(),
            delta: source.into(),
        }),
    ];
    if settled {
        payloads.push(EventV1::ProviderRequestFinished(
            ProviderRequestFinishedEvent {
                request_id: "grammar".into(),
                finish_reason: "stop".into(),
                output_digest: None,
                usage: None,
                metadata: None,
            },
        ));
    }
    for (index, payload) in payloads.into_iter().enumerate() {
        let seq = u64::try_from(index).unwrap() + 1;
        app.ingest_event(EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("grammar-{seq}"),
            seq,
            run_id: "grammar".into(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, None),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload,
        });
    }
    app
}

fn render(app: &AppState, width: u16) -> Buffer {
    render_to_buffer(app, Rect::new(0, 0, width, 40), |app, frame, _| {
        ui::render_app(frame, app)
    })
}

fn position(buffer: &Buffer, needle: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let tail = (x..buffer.area.width)
                .map(|col| buffer[(col, y)].symbol())
                .collect::<String>();
            if tail.starts_with(needle) {
                return (x, y);
            }
        }
    }
    panic!(
        "missing {needle:?}\n{}",
        crate::render_test::buffer_to_string(buffer, buffer.area.width)
    );
}

struct ClipboardGuard;
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        crate::clipboard::set_copy_override(None);
        crate::clipboard::set_copy_on_select_disabled_override(None);
    }
}

fn copy(app: &mut AppState, buffer: &Buffer, start: &str, end: &str) -> String {
    let copied = Rc::new(RefCell::new(None));
    let sink = Rc::clone(&copied);
    let _guard = ClipboardGuard;
    crate::clipboard::set_copy_on_select_disabled_override(Some(false));
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.borrow_mut() = Some(text.to_string());
        Ok(())
    })));
    let (x, y) = position(buffer, start);
    let (end_x, end_y) = position(buffer, end);
    let end_x = end_x + u16::try_from(super::display_width(end)).unwrap() - 1;
    for (kind, column, row) in [
        (MouseEventKind::Down(MouseButton::Left), x, y),
        (MouseEventKind::Drag(MouseButton::Left), end_x, end_y),
        (MouseEventKind::Up(MouseButton::Left), end_x, end_y),
    ] {
        app.handle_mouse(
            MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            buffer.area,
            None,
            None,
            None,
        );
    }
    let result = copied
        .borrow()
        .clone()
        .expect("drag must copy painted text");
    result
}

#[test]
fn nested_emphasis_combines_styles_at_public_render_seam() {
    // Given: nested inline delimiters in streamed and settled assistant text.
    for settled in [false, true] {
        let app = app("**outer *inner* end**", settled);
        for width in [24, 80] {
            // When: the public renderer paints the document.
            let buffer = render(&app, width);
            let (x, y) = position(&buffer, "inner");
            // Then: both semantic styles apply without literal marker leakage.
            assert!(buffer[(x, y)]
                .modifier
                .contains(Modifier::BOLD | Modifier::ITALIC));
            assert_eq!(position(&buffer, "outer inner end").1, y);
        }
    }
}

#[test]
fn nested_links_copy_only_their_painted_label_and_destination() {
    // Given: a wide character before a link nested inside bold/emphasis.
    for width in [24, 80] {
        let mut app = app("中 **[*bold link*](https://example.com/bold)**", true);
        // When: painted linked cells are dragged through the public mouse path.
        let buffer = render(&app, width);
        let copied = copy(&mut app, &buffer, "bold", "link");
        // Then: the selection range excludes delimiters and retains its actual URL.
        assert_eq!(copied, "bold link\n\nLinks:\nhttps://example.com/bold");
        let (x, y) = position(&buffer, "bold");
        assert!(buffer[(x, y)]
            .modifier
            .contains(Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED));
        assert_eq!(
            buffer[(x, y)].fg,
            crate::theme::Theme::default().markdown.link_text
        );
    }
}

#[test]
fn tilde_fences_highlight_and_select_code_without_markers() {
    // Given: a CommonMark tilde fence, open and closed.
    for settled in [false, true] {
        let source = if settled {
            "~~~rust\nlet value = 42;\n~~~"
        } else {
            "~~~rust\nlet value = 42;"
        };
        for width in [24, 80] {
            let mut app = app(source, settled);
            // When: the public renderer and mouse-copy path consume the same code.
            let buffer = render(&app, width);
            let copied = copy(&mut app, &buffer, "let", "42;");
            // Then: code is highlighted, not Markdown prose or fence syntax.
            assert_eq!(copied, "let value = 42;");
            let (x, y) = position(&buffer, "let");
            assert_ne!(
                buffer[(x, y)].fg,
                crate::theme::Theme::default().text.primary
            );
            assert!(!crate::render_test::buffer_to_string(&buffer, width).contains("~~~"));
        }
    }
}

#[test]
fn code_indentation_tabs_citation_syntax_and_copy_follow_the_same_wrapping() {
    for settled in [false, true] {
        let source = format!(
            "```1:4:src/main.rs\nfn main() {{\n\tlet answer = 42;\n\n}}{}",
            if settled { "\n```" } else { "" }
        );
        for width in [32, 80] {
            let mut app = app(&source, settled);
            let buffer = render(&app, width);
            let (function_x, _) = position(&buffer, "fn main");
            let (let_x, let_y) = position(&buffer, "let answer");
            assert_eq!(let_x, function_x + 4);
            assert_ne!(buffer[(let_x, let_y)].fg, app.theme().text.primary);
            assert_eq!(
                copy(&mut app, &buffer, "fn main", "}"),
                "fn main() {\n    let answer = 42;\n\n}"
            );
        }
    }
}

#[test]
fn longer_fence_keeps_shorter_and_suffixed_closers_in_code() {
    // Given: shorter, wrong-marker, and non-whitespace closing-fence candidates.
    for width in [24, 80] {
        let mut app = app("````rust\nalpha\n```\n~~~\n````bad\nomega\n`````", true);
        // When: the public renderer paints and selects the entire code body.
        let buffer = render(&app, width);
        let copied = copy(&mut app, &buffer, "alpha", "omega");
        // Then: only the valid long closing fence is consumed.
        assert_eq!(copied, "alpha\n```\n~~~\n````bad\nomega");
    }
}

#[test]
fn softbreaks_collapse_but_hardbreaks_and_paragraphs_stay_distinct() {
    // Given: CRLF softbreaks, a blank paragraph boundary, and an explicit hardbreak.
    for width in [24, 80] {
        let mut app = app("alpha\r\nbeta\n\ngamma  \ndelta", true);
        // When: pretty text is painted and selected across the whole response.
        let buffer = render(&app, width);
        position(&buffer, "alpha beta");
        let copied = copy(&mut app, &buffer, "alpha", "delta");
        // Then: only the soft source newline collapses.
        assert_eq!(copied, "alpha beta\n\ngamma\ndelta");
    }
}

#[test]
fn quote_depth_repeats_across_wrapping_without_implicit_italic_or_copy_bars() {
    // Given: nested quotes long enough to wrap at 24 columns.
    for width in [24, 80] {
        let mut app = app("> > alpha beta gamma delta epsilon zeta", true);
        // When: the public renderer lays out the nested quote.
        let buffer = render(&app, width);
        let (x, y) = position(&buffer, "alpha");
        // Then: both bars repeat on each occupied quote row; only source emphasis is italic.
        for row in y..=position(&buffer, "zeta").1 {
            assert_eq!(buffer[(x - 4, row)].symbol(), "│");
            assert_eq!(buffer[(x - 2, row)].symbol(), "│");
            assert!(!buffer[(x, row)].modifier.contains(Modifier::ITALIC));
        }
        assert_eq!(
            copy(&mut app, &buffer, "alpha", "zeta"),
            "alpha beta gamma delta epsilon zeta"
        );
    }
}

#[test]
fn terminal_math_renders_supported_forms_and_copies_the_visible_result() {
    // Given: canonical and alternate inline math plus a display fraction.
    for (source, visible) in [
        ("$E=mc^2$", "E=mc²"),
        (r"\(a^2+b^2=c^2\)", "a²+b²=c²"),
        ("$$\n\\frac{1}{2}\n$$", "½"),
    ] {
        for width in [24, 80] {
            let mut app = app(&format!("begin {source}\n\nend"), true);
            // When: math is painted and copied through the real selection path.
            let buffer = render(&app, width);
            position(&buffer, visible);
            let copied = copy(&mut app, &buffer, "begin", "end");
            // Then: selection geometry follows the transformed cells, not TeX byte offsets.
            assert!(copied.contains(visible), "{copied}");
            assert!(
                !copied.contains('$') && !copied.contains("\\frac"),
                "{copied}"
            );
        }
    }
}

#[test]
fn malformed_math_and_code_literals_remain_readable() {
    // Given: unmatched/unsupported math plus literal math inside code.
    let source = "bad $broken{\n\n$\\frac{1}{$\n\n`$E=mc^2$`\n\n~~~text\n\\(x^2\\)\n~~~";
    let mut app = app(source, true);
    // When: pretty rendering and copy consume malformed and protected input.
    let buffer = render(&app, 80);
    // Then: malformed source is not discarded, and code is never converted to math.
    for literal in ["$broken{", "\\frac{1}{", "$E=mc^2$", "\\(x^2\\)"] {
        position(&buffer, literal);
    }
    assert!(copy(&mut app, &buffer, "bad", "\\(x^2\\)").contains("$broken{"));
}

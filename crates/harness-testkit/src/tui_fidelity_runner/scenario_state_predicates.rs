use crate::parity::SemanticFrame;
use crate::tui_fidelity::SemanticState;

pub fn semantic_state_matches(
    state: SemanticState,
    frame: &SemanticFrame,
    expected_cols: u16,
    expected_rows: u16,
) -> bool {
    let text = visible_text(frame);
    match state {
        SemanticState::Rest | SemanticState::Settled => settled(&text),
        SemanticState::PromptReady | SemanticState::StartupReady => startup_ready(&text),
        SemanticState::Working => active(&text),
        SemanticState::Streaming => streaming(&text),
        SemanticState::ToolRunning => tool_running(&text),
        SemanticState::ToolDone => tool_done(&text),
        SemanticState::PermissionOpen => permission_open(&text),
        SemanticState::QuestionOpen => question_open(&text),
        SemanticState::Resized => frame.cols == expected_cols && frame.rows == expected_rows,
    }
}

pub fn semantic_state_observed(
    state: SemanticState,
    frame: &SemanticFrame,
    expected_cols: u16,
    expected_rows: u16,
    stream_len: usize,
    minimum_stream_len: Option<usize>,
) -> bool {
    let fresh_resize_rows = state != SemanticState::Resized
        || minimum_stream_len.is_some_and(|minimum| stream_len > minimum);
    fresh_resize_rows && semantic_state_matches(state, frame, expected_cols, expected_rows)
}

fn visible_text(frame: &SemanticFrame) -> String {
    frame
        .cells
        .iter()
        .filter(|cell| !cell.continuation)
        .fold(String::new(), |mut text, cell| {
            text.push_str(&cell.grapheme);
            text
        })
}

fn startup_ready(text: &str) -> bool {
    text.contains('❯')
}

fn active(text: &str) -> bool {
    streaming(text) || tool_running(text) || text.to_ascii_lowercase().contains("working")
}

fn streaming(text: &str) -> bool {
    text.contains(crate::tui_fidelity_fixture::STREAM_SENTINEL)
        || text.contains(crate::tui_fidelity_fixture::PACKET3_STREAM_REST)
        || text.contains(crate::tui_fidelity_fixture::PACKET3_STREAM_MID)
}

fn tool_running(text: &str) -> bool {
    text.contains(crate::tui_fidelity_fixture::DISCLOSURE_SENTINEL)
        && !text.contains("Packet 3 recovery complete")
}

fn tool_done(text: &str) -> bool {
    text.contains("Packet 3 recovery complete")
        || (text.contains(crate::tui_fidelity_fixture::DISCLOSURE_SENTINEL)
            && text.contains(crate::tui_fidelity_fixture::DISCLOSURE_BODY))
}

fn permission_open(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("allow once")
        || text.contains("allow always")
        || (text.contains("permission") && text.contains("deny"))
}

fn question_open(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("answer question")
        || text.contains("submit answer")
        || (text.contains("question") && text.contains("select"))
}

fn settled(text: &str) -> bool {
    startup_ready(text) && !active(text) && !permission_open(text) && !question_open(text)
}

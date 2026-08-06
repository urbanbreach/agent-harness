use std::io::Write;
use std::time::Duration;

use portable_pty::MasterPty;

use super::error::RunnerError;
use crate::tui_fidelity::{
    AdapterKind, DragAction, KeyCode, KeySpec, MouseAction, MouseButton, MousePhase,
    ScenarioAction, WheelAction, WheelDirection,
};

pub(super) fn apply_action(
    action: &ScenarioAction,
    adapter: AdapterKind,
    master: &dyn MasterPty,
    writer: &mut dyn Write,
) -> Result<(), RunnerError> {
    match action {
        ScenarioAction::TimedKey(action) => write_bytes(writer, &key_bytes(action.key), adapter),
        ScenarioAction::Paste(action) => write_bytes(writer, action.text.as_bytes(), adapter),
        ScenarioAction::Mouse(action) => write_bytes(writer, &mouse_bytes(action), adapter),
        ScenarioAction::Drag(action) => write_bytes(writer, &drag_bytes(action), adapter),
        ScenarioAction::Wheel(action) => write_bytes(writer, &wheel_bytes(action), adapter),
        ScenarioAction::Resize(action) => master
            .resize(portable_pty::PtySize {
                rows: action.viewport.rows,
                cols: action.viewport.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| RunnerError::Process {
                adapter,
                detail: format!("resize: {error}"),
            }),
        ScenarioAction::WaitForSemanticState(_) => Ok(()),
        ScenarioAction::TerminalReply(action) => {
            write_bytes(writer, action.response.as_bytes(), adapter)
        }
    }
}

pub(super) struct ExitStep {
    pub bytes: &'static [u8],
    pub dwell: Duration,
}

const GROK_EXIT_STEPS: &[ExitStep] = &[
    ExitStep {
        bytes: b"\x15",
        dwell: Duration::from_millis(100),
    },
    ExitStep {
        bytes: b"/exit\r",
        dwell: Duration::from_millis(500),
    },
    ExitStep {
        bytes: b"\x03",
        dwell: Duration::ZERO,
    },
];
const HARNESS_EXIT_STEPS: &[ExitStep] = &[
    ExitStep {
        bytes: b"\x11",
        dwell: Duration::from_millis(100),
    },
    ExitStep {
        bytes: b"\x11",
        dwell: Duration::ZERO,
    },
];

pub(super) const fn normal_exit_steps(adapter: AdapterKind) -> &'static [ExitStep] {
    match adapter {
        AdapterKind::Grok => GROK_EXIT_STEPS,
        AdapterKind::Harness => HARNESS_EXIT_STEPS,
    }
}

pub(super) fn normal_exit_timeline(adapter: AdapterKind) -> Vec<serde_json::Value> {
    normal_exit_steps(adapter)
        .iter()
        .enumerate()
        .map(|(step, exit)| {
            let bytes_hex = exit.bytes.iter().fold(String::new(), |mut value, byte| {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                value.push(char::from(HEX[usize::from(byte >> 4)]));
                value.push(char::from(HEX[usize::from(byte & 0x0f)]));
                value
            });
            serde_json::json!({
                "kind": "normal_exit_step",
                "step": step,
                "bytes_hex": bytes_hex,
                "dwell_millis": exit.dwell.as_millis(),
            })
        })
        .collect()
}

fn write_bytes(
    writer: &mut dyn Write,
    bytes: &[u8],
    adapter: AdapterKind,
) -> Result<(), RunnerError> {
    writer
        .write_all(bytes)
        .and_then(|()| writer.flush())
        .map_err(|error| RunnerError::Process {
            adapter,
            detail: format!("write input: {error}"),
        })
}

fn key_bytes(key: KeySpec) -> Vec<u8> {
    let mut bytes = match key.code {
        KeyCode::Char(character) => {
            let character = if key.modifiers.shift {
                character.to_ascii_uppercase()
            } else {
                character
            };
            if key.modifiers.ctrl && character.is_ascii() {
                vec![(character as u8) & 0x1f]
            } else {
                character.to_string().into_bytes()
            }
        }
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::Backspace => b"\x7f".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Esc => b"\x1b".to_vec(),
    };
    if key.modifiers.alt || key.modifiers.meta {
        bytes.insert(0, b'\x1b');
    }
    bytes
}

fn mouse_bytes(action: &MouseAction) -> Vec<u8> {
    let button = mouse_button_code(action.button);
    let suffix = match action.phase {
        MousePhase::Down => 'M',
        MousePhase::Up => 'm',
    };
    sgr_mouse(button, action.point.col, action.point.row, suffix)
}

fn drag_bytes(action: &DragAction) -> Vec<u8> {
    let button = mouse_button_code(action.button);
    let mut bytes = sgr_mouse(button, action.from.col, action.from.row, 'M');
    bytes.extend(sgr_mouse(button + 32, action.to.col, action.to.row, 'M'));
    bytes.extend(sgr_mouse(button, action.to.col, action.to.row, 'm'));
    bytes
}

fn wheel_bytes(action: &WheelAction) -> Vec<u8> {
    let code = match action.direction {
        WheelDirection::Up => 64,
        WheelDirection::Down => 65,
        WheelDirection::Left => 66,
        WheelDirection::Right => 67,
    };
    (0..action.amount)
        .flat_map(|_| sgr_mouse(code, action.point.col, action.point.row, 'M'))
        .collect()
}

const fn mouse_button_code(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

fn sgr_mouse(code: u8, col: u16, row: u16, suffix: char) -> Vec<u8> {
    format!("\x1b[<{code};{};{}{suffix}", col + 1, row + 1).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grok_exit_starts_with_ctrl_u_without_escape_toggle() {
        // Given: the Grok composer remains active after the themes scenario's Esc.
        // When: the runner requests normal exit.
        let steps = normal_exit_steps(AdapterKind::Grok);

        // Then: the first byte clears the composer without toggling Esc state.
        assert_eq!(steps[0].bytes, b"\x15");
    }
}

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::navigation::TimelineJump;

pub fn key_jump(key: KeyEvent) -> Option<TimelineJump> {
    match (key.modifiers, key.code) {
        (KeyModifiers::SHIFT, KeyCode::Char('J') | KeyCode::Char('j'))
        | (KeyModifiers::NONE, KeyCode::Char('J')) => Some(TimelineJump::NextResponse),
        (KeyModifiers::SHIFT, KeyCode::Char('K') | KeyCode::Char('k'))
        | (KeyModifiers::NONE, KeyCode::Char('K')) => Some(TimelineJump::PreviousResponse),
        (KeyModifiers::NONE, KeyCode::Down | KeyCode::Right) => Some(TimelineJump::NextTurn),
        (KeyModifiers::NONE, KeyCode::Up | KeyCode::Left) => Some(TimelineJump::PreviousTurn),
        (KeyModifiers::NONE, KeyCode::Char('f') | KeyCode::Char('F')) => {
            Some(TimelineJump::JumpToFailed)
        }
        (KeyModifiers::NONE, KeyCode::Char('s') | KeyCode::Char('S')) => {
            Some(TimelineJump::JumpToStreaming)
        }
        _ => None,
    }
}

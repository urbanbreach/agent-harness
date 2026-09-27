pub use super::key_navigation::key_jump;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimelineJump {
    NextTurn,
    PreviousTurn,
    NextResponse,
    PreviousResponse,
    JumpToFailed,
    JumpToStreaming,
}

pub type KeyJump = TimelineJump;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScenarioId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tick(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellPoint {
    pub col: u16,
    pub row: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellRect {
    pub col: u16,
    pub row: u16,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextStyle {
    pub foreground: Rgb,
    pub background: Rgb,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wrapping {
    NoWrap,
    HardWrap,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextPlacement {
    pub text: String,
    pub cell_width: u16,
    pub padding_left: u16,
    pub padding_right: u16,
    pub style: TextStyle,
    pub wrapping: Wrapping,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    Grok,
    Harness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyCode {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    Esc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyModifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub meta: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeySpec {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MousePhase {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticState {
    Rest,
    PromptReady,
    Working,
    Settled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalQuery {
    DeviceAttributes,
    CursorPosition,
    ModeStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimedKeyAction {
    pub at_tick: Tick,
    pub key: KeySpec,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasteAction {
    pub at_tick: Tick,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MouseAction {
    pub at_tick: Tick,
    pub button: MouseButton,
    pub phase: MousePhase,
    pub point: CellPoint,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DragAction {
    pub at_tick: Tick,
    pub button: MouseButton,
    pub from: CellPoint,
    pub to: CellPoint,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelAction {
    pub at_tick: Tick,
    pub direction: WheelDirection,
    pub amount: u16,
    pub point: CellPoint,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResizeAction {
    pub at_tick: Tick,
    pub viewport: Viewport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitForSemanticStateAction {
    pub at_tick: Tick,
    pub state: SemanticState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalReplyAction {
    pub at_tick: Tick,
    pub query: TerminalQuery,
    pub response: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioAction {
    TimedKey(TimedKeyAction),
    Paste(PasteAction),
    Mouse(MouseAction),
    Drag(DragAction),
    Wheel(WheelAction),
    Resize(ResizeAction),
    WaitForSemanticState(WaitForSemanticStateAction),
    TerminalReply(TerminalReplyAction),
}

impl ScenarioAction {
    pub fn at_tick(&self) -> Tick {
        match self {
            Self::TimedKey(action) => action.at_tick,
            Self::Paste(action) => action.at_tick,
            Self::Mouse(action) => action.at_tick,
            Self::Drag(action) => action.at_tick,
            Self::Wheel(action) => action.at_tick,
            Self::Resize(action) => action.at_tick,
            Self::WaitForSemanticState(action) => action.at_tick,
            Self::TerminalReply(action) => action.at_tick,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::TimedKey(_) => "timed_key",
            Self::Paste(_) => "paste",
            Self::Mouse(_) => "mouse",
            Self::Drag(_) => "drag",
            Self::Wheel(_) => "wheel",
            Self::Resize(_) => "resize",
            Self::WaitForSemanticState(_) => "wait_for_semantic_state",
            Self::TerminalReply(_) => "terminal_reply",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointName {
    Rest,
    Mid,
    Settled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameCapture {
    pub capture_id: String,
    pub viewport: Viewport,
    pub state: SemanticState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub name: CheckpointName,
    pub at_tick: Tick,
    pub frame: FrameCapture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityScope {
    ProductLogo,
    ProductTitle,
    BuildVersion,
    ProviderName,
    AccountName,
}

impl IdentityScope {
    pub const fn placeholder(self) -> &'static str {
        match self {
            Self::ProductLogo => "[LOGO]",
            Self::ProductTitle => "[PRODUCT]",
            Self::BuildVersion => "[VERSION]",
            Self::ProviderName => "[PROVIDER]",
            Self::AccountName => "[ACCOUNT]",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentitySubstitution {
    pub checkpoint: CheckpointName,
    pub scope: IdentityScope,
    pub rectangle: CellRect,
    pub source: TextPlacement,
    pub target: TextPlacement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedExit {
    pub code: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupExpectation {
    pub restore_workspace: bool,
    pub preserve_evidence: bool,
    pub temporary_paths: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub schema_version: String,
    pub id: ScenarioId,
    pub adapters: Vec<AdapterKind>,
    pub viewport: Viewport,
    pub actions: Vec<ScenarioAction>,
    pub checkpoints: Vec<Checkpoint>,
    pub substitutions: Vec<IdentitySubstitution>,
    pub expected_exit: ExpectedExit,
    pub cleanup: CleanupExpectation,
}

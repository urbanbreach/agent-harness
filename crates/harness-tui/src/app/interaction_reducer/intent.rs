use super::super::Focus;
use super::state::ScreenMode;
use crate::keybindings::Action;
use crate::overlay::OverlayKind;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureKind {
    TranscriptSelection,
    ScrollbarDrag,
    OverlayActivation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayTarget {
    Top,
    Kind(OverlayKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiIntent {
    MoveFocus(FocusDirection),
    SetFocus(Focus),
    OpenOverlay(OverlayKind),
    CloseOverlay(OverlayTarget),
    BeginGesture(GestureKind),
    UpdateGesture(GestureKind),
    EndGesture,
    DispatchAction(Action),
    CompleteAction(Action),
    SwitchScreen(ScreenMode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseTarget {
    FocusNext,
    FocusPrevious,
    Focus(Focus),
    Activate,
    OpenOverlay(OverlayKind),
    CloseOverlay(OverlayKind),
    BeginGesture(GestureKind),
    UpdateGesture(GestureKind),
    EndGesture,
    ScrollUp,
    ScrollDown,
}

pub fn keyboard_intent(event: KeyEvent) -> Option<UiIntent> {
    match event.code {
        KeyCode::Tab if event.modifiers == KeyModifiers::NONE => {
            Some(UiIntent::MoveFocus(FocusDirection::Next))
        }
        KeyCode::BackTab if event.modifiers == KeyModifiers::NONE => {
            Some(UiIntent::MoveFocus(FocusDirection::Previous))
        }
        KeyCode::Tab if event.modifiers.contains(KeyModifiers::SHIFT) => {
            Some(UiIntent::MoveFocus(FocusDirection::Previous))
        }
        KeyCode::Esc if event.modifiers == KeyModifiers::NONE => {
            Some(UiIntent::CloseOverlay(OverlayTarget::Top))
        }
        KeyCode::Char('p') if event.modifiers == KeyModifiers::CONTROL => {
            Some(UiIntent::OpenOverlay(OverlayKind::CommandPalette))
        }
        KeyCode::Enter if event.modifiers == KeyModifiers::NONE => {
            Some(UiIntent::DispatchAction(Action::SubmitPrompt))
        }
        _ => None,
    }
}

pub fn mouse_intent(event: MouseEvent, target: MouseTarget) -> Option<UiIntent> {
    match (event.kind, target) {
        (MouseEventKind::Down(MouseButton::Left), MouseTarget::FocusNext) => {
            Some(UiIntent::MoveFocus(FocusDirection::Next))
        }
        (MouseEventKind::Down(MouseButton::Left), MouseTarget::FocusPrevious) => {
            Some(UiIntent::MoveFocus(FocusDirection::Previous))
        }
        (MouseEventKind::Down(MouseButton::Left), MouseTarget::Focus(focus)) => {
            Some(UiIntent::SetFocus(focus))
        }
        (MouseEventKind::Up(MouseButton::Left), MouseTarget::Activate) => {
            Some(UiIntent::DispatchAction(Action::SubmitPrompt))
        }
        (MouseEventKind::Down(MouseButton::Left), MouseTarget::OpenOverlay(kind)) => {
            Some(UiIntent::OpenOverlay(kind))
        }
        (MouseEventKind::Up(MouseButton::Left), MouseTarget::CloseOverlay(kind)) => {
            Some(UiIntent::CloseOverlay(OverlayTarget::Kind(kind)))
        }
        (MouseEventKind::Down(MouseButton::Left), MouseTarget::BeginGesture(kind)) => {
            Some(UiIntent::BeginGesture(kind))
        }
        (MouseEventKind::Drag(MouseButton::Left), MouseTarget::UpdateGesture(kind)) => {
            Some(UiIntent::UpdateGesture(kind))
        }
        (MouseEventKind::Up(MouseButton::Left), MouseTarget::EndGesture) => {
            Some(UiIntent::EndGesture)
        }
        (MouseEventKind::ScrollUp, MouseTarget::ScrollUp) => {
            Some(UiIntent::DispatchAction(Action::ScrollUp))
        }
        (MouseEventKind::ScrollDown, MouseTarget::ScrollDown) => {
            Some(UiIntent::DispatchAction(Action::ScrollDown))
        }
        _ => None,
    }
}

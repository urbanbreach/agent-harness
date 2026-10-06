use super::{WelcomeHit, WelcomeRegion};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WelcomeFocus {
    Prompt,
    Menu(usize),
    StatusBar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WelcomeAction {
    NewWorktree,
    ResumeSession,
    Changelog,
    Quit,
}

impl WelcomeAction {
    pub const ALL: [Self; 4] = [
        Self::NewWorktree,
        Self::ResumeSession,
        Self::Changelog,
        Self::Quit,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::NewWorktree => "New worktree",
            Self::ResumeSession => "Resume session",
            Self::Changelog => "Changelog",
            Self::Quit => "Quit",
        }
    }

    pub const fn shortcut(self) -> &'static str {
        match self {
            Self::NewWorktree => "ctrl+w",
            Self::ResumeSession => "ctrl+s",
            Self::Changelog => "",
            Self::Quit => "ctrl+q",
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WelcomeInput {
    Select,
    MoveUp,
    MoveDown,
    FocusPrompt,
    FocusMenu,
    Activate,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputResult {
    NoOp,
    FocusChanged,
    Navigated,
    MenuItemActivated(usize),
    PromptActivated,
    Cancelled,
}

pub struct WelcomeState {
    focus: WelcomeFocus,
    hovered_hit: Option<WelcomeHit>,
    pressed_action: Option<WelcomePointerPress>,
    menu_item_count: usize,
    dismissed: bool,
    authed: bool,
    model_name: Option<String>,
    workspace_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WelcomePointerPress {
    pub action_index: usize,
    pub column: u16,
    pub row: u16,
}

impl WelcomeState {
    pub fn new(menu_item_count: usize, authed: bool) -> Self {
        Self {
            focus: WelcomeFocus::Prompt,
            hovered_hit: None,
            pressed_action: None,
            menu_item_count,
            dismissed: false,
            authed,
            model_name: None,
            workspace_name: None,
        }
    }

    pub fn focus(&self) -> WelcomeFocus {
        self.focus
    }

    pub fn menu_item_count(&self) -> usize {
        self.menu_item_count
    }

    pub fn dismiss_for_input(&mut self) {
        self.dismissed = true;
        self.focus = WelcomeFocus::Prompt;
        self.hovered_hit = None;
    }

    pub fn is_dismissed(&self) -> bool {
        self.dismissed
    }

    pub fn focus_menu_item(&mut self, index: usize) -> InputResult {
        if index >= self.menu_item_count {
            return InputResult::NoOp;
        }
        self.change_focus(WelcomeFocus::Menu(index))
    }

    pub fn hovered_action(&self) -> Option<usize> {
        self.hovered_hit
            .filter(|hit| hit.region == WelcomeRegion::Menu)
            .and_then(|hit| hit.item_index)
    }

    pub fn changelog_header_hovered(&self) -> bool {
        self.hovered_hit
            .is_some_and(|hit| hit.region == WelcomeRegion::ChangelogHeader)
    }

    pub fn set_hovered_hit(&mut self, hit: Option<WelcomeHit>) -> bool {
        let next = hit.filter(|hit| {
            hit.item_index
                .is_some_and(|index| index < self.menu_item_count)
        });
        if self.hovered_hit == next {
            return false;
        }
        self.hovered_hit = next;
        true
    }

    pub fn begin_pointer_press(&mut self, action_index: usize, column: u16, row: u16) {
        self.pressed_action = Some(WelcomePointerPress {
            action_index,
            column,
            row,
        });
    }

    pub fn take_pointer_press(&mut self) -> Option<WelcomePointerPress> {
        self.pressed_action.take()
    }

    pub fn cancel_pointer_press(&mut self) -> bool {
        self.pressed_action.take().is_some()
    }

    pub fn selected_action(&self) -> Option<WelcomeAction> {
        let WelcomeFocus::Menu(index) = self.focus else {
            return None;
        };
        WelcomeAction::from_index(index)
    }

    pub fn authed(&self) -> bool {
        self.authed
    }

    pub fn model_name(&self) -> Option<&str> {
        self.model_name.as_deref()
    }

    pub fn workspace_name(&self) -> Option<&str> {
        self.workspace_name.as_deref()
    }

    pub fn set_model(&mut self, name: Option<String>) {
        self.model_name = name;
    }

    pub fn set_workspace(&mut self, name: Option<String>) {
        self.workspace_name = name;
    }

    pub fn handle(&mut self, input: WelcomeInput) -> InputResult {
        match input {
            WelcomeInput::FocusPrompt => self.change_focus(WelcomeFocus::Prompt),
            WelcomeInput::FocusMenu if self.menu_item_count > 0 => {
                self.change_focus(WelcomeFocus::Menu(0))
            }
            WelcomeInput::FocusMenu => InputResult::NoOp,
            WelcomeInput::MoveUp | WelcomeInput::MoveDown => self.move_menu(input),
            WelcomeInput::Activate => match self.focus {
                WelcomeFocus::Menu(index) => InputResult::MenuItemActivated(index),
                WelcomeFocus::Prompt => InputResult::PromptActivated,
                WelcomeFocus::StatusBar => InputResult::NoOp,
            },
            WelcomeInput::Cancel => InputResult::Cancelled,
            WelcomeInput::Select => self.select_next(),
        }
    }

    fn change_focus(&mut self, focus: WelcomeFocus) -> InputResult {
        if self.focus == focus {
            InputResult::NoOp
        } else {
            self.focus = focus;
            InputResult::FocusChanged
        }
    }

    fn move_menu(&mut self, input: WelcomeInput) -> InputResult {
        let WelcomeFocus::Menu(index) = self.focus else {
            return InputResult::NoOp;
        };
        if self.menu_item_count == 0 {
            return InputResult::NoOp;
        }
        let next = match input {
            WelcomeInput::MoveUp => index.checked_sub(1).unwrap_or(self.menu_item_count - 1),
            WelcomeInput::MoveDown => index.saturating_add(1) % self.menu_item_count,
            _ => index,
        };
        self.focus = WelcomeFocus::Menu(next);
        InputResult::Navigated
    }

    fn select_next(&mut self) -> InputResult {
        let next = match self.focus {
            WelcomeFocus::Prompt => {
                if self.menu_item_count > 0 {
                    WelcomeFocus::Menu(0)
                } else {
                    WelcomeFocus::StatusBar
                }
            }
            WelcomeFocus::Menu(_) => WelcomeFocus::StatusBar,
            WelcomeFocus::StatusBar => WelcomeFocus::Prompt,
        };
        self.focus = next;
        InputResult::FocusChanged
    }
}

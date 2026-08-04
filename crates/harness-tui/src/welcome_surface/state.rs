#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WelcomeFocus {
    Prompt,
    Menu(usize),
    StatusBar,
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
    menu_item_count: usize,
    authed: bool,
    model_name: Option<String>,
    workspace_name: Option<String>,
}

impl WelcomeState {
    pub fn new(menu_item_count: usize, authed: bool) -> Self {
        Self {
            focus: WelcomeFocus::Prompt,
            menu_item_count,
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

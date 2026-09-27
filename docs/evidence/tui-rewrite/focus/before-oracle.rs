
#![allow(dead_code,unused_imports)]
#[derive(Clone,Copy,Debug,PartialEq)] enum Focus { List, Details, Terminal, Prompt }
#[derive(Clone,Copy,PartialEq)] enum Tab { Run }
#[derive(Clone,Copy)] enum ReviewSurface { Help }
mod welcome_surface { pub enum WelcomeInput { FocusMenu, FocusPrompt } }
#[derive(Default)] struct Welcome { menu: Option<bool>, calls: usize }
impl Welcome { fn handle(&mut self, input: welcome_surface::WelcomeInput) {
    self.menu=Some(matches!(input,welcome_surface::WelcomeInput::FocusMenu)); self.calls+=1;
} }
struct AppState { focus: Focus, replay_mode: bool, startup: bool, active_tab: Tab,
    terminal: bool, active_review_surface: Option<ReviewSurface>, live_details_drawer_open: bool,
    welcome: Welcome }
impl AppState {
    fn startup_shell_visible(&self)->bool { !self.replay_mode && self.startup }
    fn post_run_handoff_visible(&self)->bool { false }
    fn terminal_panel_visible(&self)->bool { self.terminal }
    fn session_shell_operator_rail_interactive(&self)->bool {
        !self.replay_mode && self.active_tab==Tab::Run && self.live_details_drawer_open
    }
}
impl AppState {
    fn normalize_focus_for_active_surface(&mut self) {
        if self.replay_mode {
            if self.focus == Focus::Prompt {
                self.focus = if self.session_shell_operator_rail_interactive() {
                    Focus::List
                } else {
                    Focus::Details
                };
            } else if (self.focus == Focus::Terminal && !self.terminal_panel_visible())
                || (self.active_review_surface.is_none()
                    && !self.session_shell_operator_rail_interactive()
                    && self.focus == Focus::List)
            {
                self.focus = Focus::Details;
            }
            return;
        }

        if self.post_run_handoff_visible() {
            if matches!(self.focus, Focus::Prompt | Focus::Terminal) || self.active_tab == Tab::Run
            {
                self.focus = Focus::List;
            }
            return;
        }

        if self.startup_shell_visible() {
            if self.focus == Focus::Prompt {
                self.focus = Focus::List;
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusMenu);
            } else {
                self.focus = Focus::Prompt;
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusPrompt);
            }
            return;
        }

        if self.active_review_surface.is_some() && self.focus == Focus::Prompt {
            self.focus = Focus::List;
        } else if (self.active_review_surface.is_some() && self.focus == Focus::Terminal)
            || (self.active_review_surface.is_none()
                && !self.startup_shell_visible()
                && !self.session_shell_operator_rail_interactive()
                && self.focus == Focus::List)
        {
            self.focus = Focus::Details;
        }
    }
    fn cycle_focus_forward(&mut self) {
        if self.replay_mode {
            if !self.session_shell_operator_rail_interactive() {
                self.focus = Focus::Details;
                return;
            }

            self.focus = match self.focus {
                Focus::List => Focus::Details,
                Focus::Details if self.terminal_panel_visible() => Focus::Terminal,
                Focus::Terminal | Focus::Details | Focus::Prompt => Focus::List,
            };
            return;
        }

        if self.post_run_handoff_visible() {
            self.focus = if self.active_tab == Tab::Run {
                Focus::List
            } else {
                match self.focus {
                    Focus::List | Focus::Prompt | Focus::Terminal => Focus::Details,
                    Focus::Details => Focus::List,
                }
            };
            return;
        }

        if self.startup_shell_visible() {
            if self.focus == Focus::Prompt {
                self.focus = Focus::List;
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusMenu);
            } else {
                self.focus = Focus::Prompt;
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusPrompt);
            }
            return;
        }

        if self.active_review_surface.is_none()
            && !self.startup_shell_visible()
            && !self.session_shell_operator_rail_interactive()
        {
            self.focus = match self.focus {
                Focus::Prompt => Focus::Details,
                Focus::Details if self.terminal_panel_visible() => Focus::Terminal,
                Focus::Terminal | Focus::Details | Focus::List => Focus::Prompt,
            };
            self.live_details_drawer_open = false;
            return;
        }

        self.focus = if self.active_review_surface.is_none() {
            match self.focus {
                Focus::Details => Focus::List,
                Focus::List => Focus::Prompt,
                Focus::Terminal => Focus::Prompt,
                Focus::Prompt => Focus::Details,
            }
        } else {
            match self.focus {
                Focus::List => Focus::Details,
                Focus::Details | Focus::Terminal => Focus::Prompt,
                Focus::Prompt => Focus::List,
            }
        };

        if self.active_review_surface.is_none() {
            self.live_details_drawer_open = self.focus == Focus::List;
        }
    }
    fn cycle_focus_backward(&mut self) {
        if self.replay_mode {
            if !self.session_shell_operator_rail_interactive() {
                self.focus = if self.focus == Focus::Terminal {
                    Focus::Details
                } else if self.terminal_panel_visible() {
                    Focus::Terminal
                } else {
                    Focus::Details
                };
                return;
            }

            self.focus = match self.focus {
                Focus::List | Focus::Prompt => {
                    if self.terminal_panel_visible() {
                        Focus::Terminal
                    } else {
                        Focus::Details
                    }
                }
                Focus::Terminal => Focus::Details,
                Focus::Details => Focus::List,
            };
            return;
        }

        if self.post_run_handoff_visible() {
            self.focus = if self.active_tab == Tab::Run {
                Focus::List
            } else {
                match self.focus {
                    Focus::List | Focus::Prompt | Focus::Terminal => Focus::Details,
                    Focus::Details => Focus::List,
                }
            };
            return;
        }

        if self.startup_shell_visible() {
            if self.focus == Focus::Prompt {
                self.focus = Focus::List;
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusMenu);
            } else {
                self.focus = Focus::Prompt;
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusPrompt);
            }
            return;
        }

        if self.active_review_surface.is_none()
            && !self.startup_shell_visible()
            && !self.session_shell_operator_rail_interactive()
        {
            self.focus = match self.focus {
                Focus::Prompt if self.terminal_panel_visible() => Focus::Terminal,
                Focus::Prompt => Focus::Details,
                Focus::Terminal => Focus::Details,
                Focus::Details => Focus::Prompt,
                Focus::List => Focus::Details,
            };
            self.live_details_drawer_open = false;
            return;
        }

        self.focus = if self.active_review_surface.is_none() {
            match self.focus {
                Focus::Details if self.terminal_panel_visible() => Focus::Terminal,
                Focus::Details => Focus::Prompt,
                Focus::Terminal => Focus::Prompt,
                Focus::List => Focus::Details,
                Focus::Prompt => Focus::List,
            }
        } else {
            match self.focus {
                Focus::List => Focus::Prompt,
                Focus::Details | Focus::Terminal => Focus::List,
                Focus::Prompt => Focus::Details,
            }
        };

        if self.active_review_surface.is_none() {
            self.live_details_drawer_open = self.focus == Focus::List;
        }
    }
}fn main() {
    for mode in 0..3 { for review in [false,true] { for drawer in [false,true] {
    for terminal in [false,true] { for focus in [Focus::List,Focus::Details,Focus::Terminal,Focus::Prompt] {
    for action in 0..3 {
        let mut app=AppState {focus, replay_mode:mode==2,startup:mode==1,
            active_tab:Tab::Run,terminal,active_review_surface:review.then_some(ReviewSurface::Help),
            live_details_drawer_open:drawer,welcome:Welcome::default()};
        match action {0=>app.normalize_focus_for_active_surface(),1=>app.cycle_focus_forward(),_=>app.cycle_focus_backward()}
        println!("{mode},{review},{drawer},{terminal},{focus:?},{action}|{:?},{},{:?},{}",
            app.focus,app.live_details_drawer_open,app.welcome.menu,app.welcome.calls);
    } } } } } }
}

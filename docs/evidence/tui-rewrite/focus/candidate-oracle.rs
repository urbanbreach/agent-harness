
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
            if self.focus == Focus::Prompt
                || (self.focus == Focus::Terminal && !self.terminal_panel_visible())
                || (self.focus == Focus::List && self.active_review_surface.is_none())
            {
                self.focus = Focus::Details;
            }
        } else if self.startup_shell_visible() {
            self.toggle_startup_focus();
        } else {
            self.focus = match self.focus {
                Focus::Prompt if self.active_review_surface.is_some() => Focus::List,
                Focus::Terminal if self.active_review_surface.is_some() => Focus::Details,
                Focus::List
                    if self.active_review_surface.is_none()
                        && !self.session_shell_operator_rail_interactive() =>
                {
                    Focus::Details
                }
                focus => focus,
            };
        }
    }

    fn cycle_focus(&mut self, backward: bool) {
        use Focus::{Details, List, Prompt, Terminal};
        if self.replay_mode {
            // Default Tab keys are consumed earlier; remapped reverse focus can
            // still reach the read-only terminal panel.
            self.focus = if backward && self.focus != Terminal && self.terminal_panel_visible() {
                Terminal
            } else {
                Details
            };
            return;
        }
        if self.startup_shell_visible() {
            self.toggle_startup_focus();
            return;
        }
        if self.active_review_surface.is_some() {
            self.focus = match (self.focus, backward) {
                (List, false) | (Prompt, true) => Details,
                (Prompt, false) | (Details | Terminal, true) => List,
                _ => Prompt,
            };
            return;
        }

        self.focus = if self.session_shell_operator_rail_interactive() {
            match (self.focus, backward) {
                (Details, false) | (Prompt, true) => List,
                (List, true) | (Prompt, false) => Details,
                (Details, true) if self.terminal_panel_visible() => Terminal,
                _ => Prompt,
            }
        } else {
            match (self.focus, backward) {
                (Details, false) | (Prompt, true) if self.terminal_panel_visible() => Terminal,
                (Prompt, _) | (Terminal | List, true) => Details,
                _ => Prompt,
            }
        };
        self.live_details_drawer_open = self.focus == List;
    }

    fn toggle_startup_focus(&mut self) {
        use crate::welcome_surface::WelcomeInput;
        let input = if self.focus == Focus::Prompt {
            self.focus = Focus::List;
            WelcomeInput::FocusMenu
        } else {
            self.focus = Focus::Prompt;
            WelcomeInput::FocusPrompt
        };
        self.welcome.handle(input);
    }
}
fn main() {
    for mode in 0..3 { for review in [false,true] { for drawer in [false,true] {
    for terminal in [false,true] { for focus in [Focus::List,Focus::Details,Focus::Terminal,Focus::Prompt] {
    for action in 0..3 {
        let mut app=AppState {focus, replay_mode:mode==2,startup:mode==1,
            active_tab:Tab::Run,terminal,active_review_surface:review.then_some(ReviewSurface::Help),
            live_details_drawer_open:drawer,welcome:Welcome::default()};
        match action {0=>app.normalize_focus_for_active_surface(),1=>app.cycle_focus(false),_=>app.cycle_focus(true)}
        println!("{mode},{review},{drawer},{terminal},{focus:?},{action}|{:?},{},{:?},{}",
            app.focus,app.live_details_drawer_open,app.welcome.menu,app.welcome.calls);
    } } } } } }
}

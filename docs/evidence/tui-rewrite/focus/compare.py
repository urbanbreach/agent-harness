from pathlib import Path
import hashlib, itertools, json, subprocess
root=Path.cwd(); out=Path(__file__).parent

def method(source, name):
    start=source.rfind('    ',0,source.index('fn '+name+'('))
    brace=source.index('{',start); depth=1;end=brace+1
    while depth:
        if source[end]=='{':depth+=1
        if source[end]=='}':depth-=1
        end+=1
    return source[start:end]

before=subprocess.check_output(['git','show','7e720aebb22f1e062f0e1bf91d8c31bb69b8ceb4:crates/harness-tui/src/app/key_interaction.rs'],cwd=root,text=True)
current=(root/'crates/harness-tui/src/app/focus.rs').read_text()
prelude='''
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
'''
main='''fn main() {
    for mode in 0..3 { for review in [false,true] { for drawer in [false,true] {
    for terminal in [false,true] { for focus in [Focus::List,Focus::Details,Focus::Terminal,Focus::Prompt] {
    for action in 0..3 {
        let mut app=AppState {focus, replay_mode:mode==2,startup:mode==1,
            active_tab:Tab::Run,terminal,active_review_surface:review.then_some(ReviewSurface::Help),
            live_details_drawer_open:drawer,welcome:Welcome::default()};
        ACTION
        println!("{mode},{review},{drawer},{terminal},{focus:?},{action}|{:?},{},{:?},{}",
            app.focus,app.live_details_drawer_open,app.welcome.menu,app.welcome.calls);
    } } } } } }
}
'''
for name, implementation, action in [
    ('before','impl AppState {\n'+ '\n'.join(method(before,n) for n in
        ['normalize_focus_for_active_surface','cycle_focus_forward','cycle_focus_backward'])+'\n}',
        'match action {0=>app.normalize_focus_for_active_surface(),1=>app.cycle_focus_forward(),_=>app.cycle_focus_backward()}'),
    ('candidate',current.replace('use super::{AppState, Focus};',''),
        'match action {0=>app.normalize_focus_for_active_surface(),1=>app.cycle_focus(false),_=>app.cycle_focus(true)}')]:
    source=(prelude+implementation+main.replace('ACTION',action)).replace('pub(in crate::app) ', '').replace('pub(super) ', '')
    (out/(name+'-oracle.rs')).write_text(source)
    subprocess.run(['rustc','--edition=2021',str(out/(name+'-oracle.rs')),'-o',str(out/(name+'-oracle'))],check=True)
    capture=subprocess.check_output([str(out/(name+'-oracle'))])
    (out/(name+'-transitions.txt')).write_bytes(capture)
old=(out/'before-transitions.txt').read_bytes(); new=(out/'candidate-transitions.txt').read_bytes()
result={'cases':len(old.splitlines()),'equal':old==new,'before_sha256':hashlib.sha256(old).hexdigest(),'candidate_sha256':hashlib.sha256(new).hexdigest(),
    'scope':'Pure focus transitions with production method bodies; stubs mirror traced lifecycle/terminal predicates. Public key routing is checked separately with nextest.'}
(out/'transitions.json').write_text(json.dumps(result,indent=2)+'\n');print(result)
assert old==new

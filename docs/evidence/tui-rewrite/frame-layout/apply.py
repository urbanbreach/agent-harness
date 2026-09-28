from pathlib import Path
import re
out=Path(__file__).resolve().parent;root=Path('crates/harness-tui/src');old=(out/'legacy/layout.rs').read_text()
head=old[:old.index('impl FrameLayoutPlan {')]
head=head.replace('// allow: SIZE_OK — TUI layout math (frame plan + pane sizing)\n','').replace('use crate::UnwrapOrAbort;\n','').replace('use ratatui::layout::{Constraint, Direction, Layout, Rect};','use ratatui::layout::Rect;').replace('use crate::app::{AppState, Focus};','use crate::app::AppState;').replace('mod surfaces;','mod surfaces;\nmod session;').replace('use overlays::{fork_selector_overlay_height, lifecycle_overlay_area};','use overlays::fork_selector_overlay_height;')
for name in ['LIVE_DETAILS_MIN_TRANSCRIPT_WIDTH','TERMINAL_PANEL_MIN_TRANSCRIPT_HEIGHT','TERMINAL_PANEL_MIN_HEIGHT','TERMINAL_PANEL_MAX_HEIGHT','STARTUP_BORDERED_COMPOSER_CHROME_ROWS','STARTUP_COMPOSER_SPACER_ROWS','STARTUP_FOOTER_ROWS','SUBAGENT_FOOTER_ROWS']:
 head=re.sub(r'^const '+name+r':[^\n]+\n','',head,flags=re.M)
head=head[:head.index('/// Spacer between')] + head[head.index('/// The native chat shell'):]
a=head.index('fn startup_composer_horizontal_inset');b=head.index('fn inset_composer_width',a);head=head[:a]+head[b:]
a=head.index('#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub(crate) struct SessionShellLayout');b=head.index('#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub struct FrameLayoutPlan',a);head=head[:a]+head[b:]
contract='''pub(crate) fn session_geometry_contract(area: Rect, shell: LiveShellLayout) -> SessionGeometryContract {
    let mode = session_responsive_mode(area, shell);
    SessionGeometryContract {
        header_mode: SessionHeaderMode::Hidden,
        footer_mode: match mode {
            SessionResponsiveMode::Dense => SessionFooterMode::Minimal,
            SessionResponsiveMode::CompactMinimum => SessionFooterMode::Reduced,
            _ => SessionFooterMode::Standard,
        },
        sidebar_mode: if mode == SessionResponsiveMode::Dense { SessionSidebarMode::Hidden } else { SessionSidebarMode::Overlay { width: shell.details_sidebar_width } },
        palette_overlay_max_width: (mode == SessionResponsiveMode::Dense).then_some(DENSE_SESSION_PALETTE_MAX_WIDTH),
        slash_overlay_max_width: None,
    }
}

'''
responsive=old[old.index('pub fn session_responsive_mode'):old.index('fn hide_session_header')]
composer=old[old.index('pub(crate) fn composer_input_height'):old.index('fn live_dock_rhythm')]
close=old[old.index('pub(crate) fn todo_close_rect'):old.index('fn terminal_panel_split')]
center=old[old.index('fn centered_live_shell_area'):old.index('#[cfg(test)]\n#[path = "layout_live_dock_test_fixtures.rs"]')]
testmods='''#[cfg(test)]
#[path = "layout_live_dock_test_fixtures.rs"]
mod live_dock_test_fixtures;
#[cfg(test)]
#[path = "layout_live_dock_tests.rs"]
mod live_dock_tests;
#[cfg(test)]
mod tests;
'''
(root/'layout.rs').write_text(head+(out/'frame-impl.rs').read_text()+'\n'+contract+responsive+composer+close+center+testmods)
tests=old[old.index('#[cfg(test)]\nmod tests {')+len('#[cfg(test)]\nmod tests {\n'):].rstrip()[:-1]
tests='\n'.join(line[4:] if line.startswith('    ') else line for line in tests.splitlines())+'\n'
a=tests.index('#[test]\nfn lifecycle_overlay_stays_centered');b=tests.index('#[test]',a+len('#[test]'));tests=tests[:a]+tests[b:]
(root/'layout/tests.rs').write_text('use crate::app::Focus;\n'+tests)
(root/'layout/session.rs').write_bytes((out/'session.rs').read_bytes())
(root/'layout/surfaces.rs').write_text((out/'legacy/layout/surfaces.rs').read_text()+(out/'surface-additions.rs').read_text())
p=root/'layout/overlays.rs';s=p.read_text();s=s[:s.index('#[cfg(test)]\npub(super) fn lifecycle_overlay_area')].rstrip()+'\n';s=s.replace('#[cfg(test)]\nuse crate::theme::LifecycleSurfaceLayout;\n\n','');p.write_text(s)
p=root/'layout_live_dock_tests.rs';s=p.read_text();s=s[:s.index('fn assert_active_rows')]+s[s.index('#[test]\nfn permission_suppression'):];p.write_text(s)
p=root/'layout_live_dock_test_fixtures.rs';s=p.read_text();s=s[:s.index('pub(super) fn interruptible_waiting_app')]+s[s.index('pub(super) fn permission_app'):];s=s.replace('    TaskScheduleState, TaskScheduledEvent, TaskTerminalScope, ToolCallRequestedEvent,\n    ToolCallStartedEvent, SCHEMA_VERSION,','    TaskTerminalScope, SCHEMA_VERSION,')
for field in ['status','composer','disclosure']:
 s=re.sub(r'^    pub\(super\) '+field+r': u16,\n','',s,flags=re.M);s=re.sub(r'^        '+field+r': \d+,\n','',s,flags=re.M)
p.write_text(s)

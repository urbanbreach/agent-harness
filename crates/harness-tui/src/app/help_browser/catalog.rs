use crate::keybindings::{Action, KeyMap};

use super::{HelpBrowserState, HelpRow, HelpSection};
use crate::app::Focus;

pub(crate) fn build_rows(
    keymap: &KeyMap,
    focus: Focus,
    replay_mode: bool,
    state: &HelpBrowserState,
) -> Vec<HelpRow> {
    let query = state.query.trim().to_ascii_lowercase();
    let searching = state.search_active || !query.is_empty();
    let mut rows = Vec::new();
    for section in HelpSection::ALL {
        let shortcuts = keymap
            .bound_actions()
            .into_iter()
            .filter(|action| action.help_category() == Some(section))
            .filter_map(|action| {
                let label = action.metadata_label();
                let description = action.metadata_description();
                if label.is_empty() || description.is_empty() {
                    return None;
                }
                let bindings = keymap.get_binding_strs(action).join(" / ");
                let dimmed = action_dimmed(section, focus, replay_mode);
                let matches_query = query.is_empty()
                    || label.to_ascii_lowercase().contains(&query)
                    || description.to_ascii_lowercase().contains(&query)
                    || bindings.to_ascii_lowercase().contains(&query);
                (matches_query && (!state.hide_dimmed || !dimmed)).then(|| HelpRow::Shortcut {
                    action,
                    label: label.to_owned(),
                    bindings,
                    description: description.to_owned(),
                    dimmed,
                    expanded: state.expanded_actions.contains(&action),
                })
            })
            .collect::<Vec<_>>();
        if shortcuts.is_empty() {
            continue;
        }
        let collapsed = !searching && state.collapsed_sections.contains(&section);
        rows.push(HelpRow::Section {
            section,
            count: shortcuts.len(),
            collapsed,
        });
        if !collapsed {
            rows.extend(shortcuts);
        }
    }
    rows
}

const fn action_dimmed(section: HelpSection, focus: Focus, replay_mode: bool) -> bool {
    match section {
        HelpSection::Input => replay_mode || !matches!(focus, Focus::Prompt),
        HelpSection::ConversationNavigation => matches!(focus, Focus::Prompt),
        HelpSection::Dashboard => !matches!(focus, Focus::List),
        HelpSection::Essentials
        | HelpSection::ConversationActions
        | HelpSection::Panels
        | HelpSection::Session => false,
    }
}

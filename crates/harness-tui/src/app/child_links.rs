use super::*;

impl AppState {
    pub(super) fn handle_child_link_key(&mut self, key: KeyEvent) -> bool {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return false;
        }
        match key.code {
            KeyCode::Char('o' | 'O') => {
                let links = self
                    .transcript_view
                    .hyperlinks
                    .iter()
                    .enumerate()
                    .filter(|(index, link)| *index == 0 || !link.continues_previous)
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                let count = links.len();
                self.transcript_view.highlighted_link = if count == 0 {
                    None
                } else {
                    let forward = key.code == KeyCode::Char('o');
                    let current = self
                        .transcript_view
                        .highlighted_link
                        .and_then(|selected| links.iter().position(|index| *index == selected));
                    let index = match (current, forward) {
                        (None, true) => 0,
                        (None, false) => count - 1,
                        (Some(index), true) => (index + 1) % count,
                        (Some(index), false) => (index + count - 1) % count,
                    };
                    Some(links[index])
                };
                true
            }
            KeyCode::Enter => {
                let Some(link) = self
                    .transcript_view
                    .highlighted_link
                    .and_then(|index| self.transcript_view.hyperlinks.get(index))
                else {
                    return false;
                };
                if crate::transcript_selection::safe_external_url(&link.destination) {
                    self.transcript_view.link_to_open = Some(link.destination.clone());
                }
                self.transcript_view.highlighted_link = None;
                true
            }
            _ => false,
        }
    }

    /// Take the explicit link-open request produced by native user input.
    /// The terminal runtime owns launching the external browser.
    pub fn take_link_to_open(&mut self) -> Option<String> {
        self.transcript_view.link_to_open.take()
    }
}

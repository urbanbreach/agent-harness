use super::*;
use crate::keybindings;

impl AppState {
    pub(in crate::app) fn apply_pending_prompt_commands(&mut self) {
        if let Some(mut discovery) = pending_live::take_pending_prompt_commands() {
            let reserved = keybindings::reserved_slash_names();
            discovery.commands.retain(|command| {
                if reserved.contains(&command.name.as_str()) {
                    discovery.warnings.push(format!(
                        "Skipping /{}: reserved by a built-in command",
                        command.name
                    ));
                    false
                } else {
                    true
                }
            });
            self.prompt_commands = discovery.commands;
            if !discovery.warnings.is_empty() {
                self.show_toast(discovery.warnings.join("\n"), ToastVariant::Warning);
            }
        }
    }

    pub(in crate::app) fn expand_prompt_command(&mut self) {
        if let Some(text) = harness_core::commands::expand_input(
            &self.prompt_commands,
            &self.composer.prompt_buffer,
        ) {
            self.replace_prompt_input(text);
            self.clear_slash_menu();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;
    use std::sync::{Arc, Mutex};

    #[test]
    fn markdown_command_enter_submits_expansion_without_replacing_login(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let project = tempfile::tempdir()?;
        let home = tempfile::tempdir()?;
        let directory = project.path().join(".harness/commands");
        std::fs::create_dir_all(&directory)?;
        std::fs::write(
            directory.join("hi.md"),
            "---\ndescription: Say hello\nargument-hint: '<who>'\n---\nHello $1",
        )?;
        std::fs::write(directory.join("login.md"), "Not a login dialog")?;
        let discovery = harness_core::commands::discover(project.path(), Some(home.path()), &[]);
        set_pending_prompt_commands(discovery.clone());
        let mut startup = AppState::new_startup(Vec::new(), None);
        assert_eq!(
            startup.toast().map(|toast| toast.variant),
            Some(ToastVariant::Warning)
        );
        assert!(startup.status_banner.is_none());
        startup.initialize_provider_connection();
        assert_eq!(startup.status_banner.as_deref(), Some(NO_PROVIDER_BANNER));
        set_pending_prompt_commands(discovery);
        let intents = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&intents);
        let mut app = AppState::new_live(
            None,
            false,
            Some(Arc::new(move |intent| {
                captured.lock().unwrap_or_abort().push(intent)
            })),
        );
        assert_eq!(
            app.toast().map(|toast| toast.variant),
            Some(ToastVariant::Warning)
        );
        assert!(app.status_banner.is_none());
        for character in "/hi".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert!(app.slash_filtered.iter().any(|name| name == "hi"));
        let index = app
            .slash_filtered
            .iter()
            .position(|name| name == "hi")
            .ok_or("missing completion")?;
        assert_eq!(app.slash_completion_label(index), "/hi <who>");
        assert_eq!(app.slash_completion_description(index), "Say hello");
        assert!(palette_controller::compute_palette_rows(&app, "")
            .iter()
            .any(|row| row.value == "prompt-command:hi"));
        for character in " there".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            matches!(intents.lock().unwrap_or_abort().last(), Some(UiIntent::SubmitPrompt { text, .. }) if text == "Hello there")
        );
        for character in "/login".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.connect_dialog.visible);
        assert_eq!(intents.lock().unwrap_or_abort().len(), 1);
        Ok(())
    }
}

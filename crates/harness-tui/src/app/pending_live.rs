use crate::UnwrapOrAbort;
#[cfg(test)]
use std::cell::RefCell;
#[cfg(not(test))]
use std::sync::MutexGuard;
#[cfg(not(test))]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

use std::path::PathBuf;

use super::{ConnectProviderOption, LaunchMetadata};
use crate::text::has_trimmed_content;

#[cfg(not(test))]
static PENDING_LIVE_LAUNCH_METADATA: Mutex<Option<LaunchMetadata>> = Mutex::new(None);
#[cfg(not(test))]
static PENDING_LIVE_PROMPT_DRAFT: Mutex<Option<String>> = Mutex::new(None);
#[cfg(not(test))]
static PENDING_LIVE_PROMPT_AUTO_SUBMIT: AtomicBool = AtomicBool::new(false);
#[cfg(not(test))]
static PENDING_LIVE_PROMPT_ENV_CONSUMED: AtomicBool = AtomicBool::new(false);
#[cfg(not(test))]
static PENDING_CONNECT_PROVIDERS: Mutex<Vec<ConnectProviderOption>> = Mutex::new(Vec::new());
#[cfg(not(test))]
static PENDING_SETTINGS_PROJECT_CONFIG: Mutex<Option<PendingSettingsProjectConfig>> =
    Mutex::new(None);

#[cfg(not(test))]
const PENDING_LIVE_PROMPT_DRAFT_ENV: &str = "HARNESS_TUI_PENDING_LIVE_PROMPT_DRAFT";
#[cfg(not(test))]
const PENDING_LIVE_PROMPT_AUTO_SUBMIT_ENV: &str = "HARNESS_TUI_PENDING_LIVE_PROMPT_AUTO_SUBMIT";

#[cfg(test)]
thread_local! {
    static PENDING_LIVE_LAUNCH_METADATA: RefCell<Option<LaunchMetadata>> = const { RefCell::new(None) };
    static PENDING_LIVE_PROMPT_DRAFT: RefCell<Option<String>> = const { RefCell::new(None) };
    static PENDING_LIVE_PROMPT_AUTO_SUBMIT: RefCell<bool> = const { RefCell::new(false) };
    static PENDING_CONNECT_PROVIDERS: RefCell<Vec<ConnectProviderOption>> = const { RefCell::new(Vec::new()) };
    static PENDING_SETTINGS_PROJECT_CONFIG: RefCell<Option<PendingSettingsProjectConfig>> =
        const { RefCell::new(None) };
}

/// Project runtime config binding for the settings editor write path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSettingsProjectConfig {
    pub path: PathBuf,
    pub hashline_edit: bool,
    pub compaction_enabled: bool,
    pub compaction_auto_retry_overflow: bool,
    pub compaction_structured_summary_contract: bool,
    pub compaction_estimated_token_triggers: bool,
    pub deterministic_enabled: bool,
}

pub(super) struct PendingLivePrompt {
    pub(super) text: String,
    pub(super) auto_submit: bool,
}

struct PendingLiveState;

impl PendingLiveState {
    #[cfg(not(test))]
    fn lock<T>(mutex: &'static Mutex<T>, _label: &str) -> MutexGuard<'static, T> {
        mutex.lock().unwrap_or_abort()
    }

    fn set_launch_metadata(metadata: LaunchMetadata) {
        #[cfg(test)]
        {
            PENDING_LIVE_LAUNCH_METADATA.with(|pending| {
                *pending.borrow_mut() = Some(metadata);
            });
        }

        #[cfg(not(test))]
        {
            *Self::lock(&PENDING_LIVE_LAUNCH_METADATA, "launch metadata") = Some(metadata);
        }
    }

    fn take_launch_metadata() -> Option<LaunchMetadata> {
        #[cfg(test)]
        {
            PENDING_LIVE_LAUNCH_METADATA.with(|pending| pending.borrow_mut().take())
        }

        #[cfg(not(test))]
        {
            Self::lock(&PENDING_LIVE_LAUNCH_METADATA, "launch metadata").take()
        }
    }

    fn set_prompt(prompt: Option<String>, auto_submit: bool) {
        #[cfg(test)]
        {
            PENDING_LIVE_PROMPT_DRAFT.with(|pending| {
                *pending.borrow_mut() = prompt;
            });
            PENDING_LIVE_PROMPT_AUTO_SUBMIT.with(|pending| {
                *pending.borrow_mut() = auto_submit;
            });
        }

        #[cfg(not(test))]
        {
            *Self::lock(&PENDING_LIVE_PROMPT_DRAFT, "prompt draft") = prompt;
            PENDING_LIVE_PROMPT_AUTO_SUBMIT.store(auto_submit, Ordering::Relaxed);
        }
    }

    fn take_prompt() -> Option<PendingLivePrompt> {
        #[cfg(test)]
        let draft = PENDING_LIVE_PROMPT_DRAFT.with(|pending| pending.borrow_mut().take());
        #[cfg(not(test))]
        let draft = Self::lock(&PENDING_LIVE_PROMPT_DRAFT, "prompt draft").take();

        #[cfg(test)]
        let auto_submit = PENDING_LIVE_PROMPT_AUTO_SUBMIT
            .with(|pending| std::mem::take(&mut *pending.borrow_mut()));
        #[cfg(not(test))]
        let auto_submit = PENDING_LIVE_PROMPT_AUTO_SUBMIT.swap(false, Ordering::Relaxed);

        if let Some(text) = draft {
            return Some(PendingLivePrompt { text, auto_submit });
        }

        #[cfg(not(test))]
        {
            if PENDING_LIVE_PROMPT_ENV_CONSUMED.swap(true, Ordering::Relaxed) {
                return None;
            }

            let draft = non_empty_prompt(std::env::var(PENDING_LIVE_PROMPT_DRAFT_ENV).ok());
            let auto_submit = std::env::var(PENDING_LIVE_PROMPT_AUTO_SUBMIT_ENV)
                .ok()
                .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
                .unwrap_or(false);
            draft.map(|text| PendingLivePrompt { text, auto_submit })
        }

        #[cfg(test)]
        {
            None
        }
    }
}

pub fn set_pending_live_launch_metadata(metadata: LaunchMetadata) {
    PendingLiveState::set_launch_metadata(metadata);
}

/// Stage project runtime config for the next live AppState so settings editor can persist.
pub fn set_pending_settings_project_config(
    path: PathBuf,
    hashline_edit: bool,
    compaction_enabled: bool,
    compaction_auto_retry_overflow: bool,
    compaction_structured_summary_contract: bool,
    compaction_estimated_token_triggers: bool,
    deterministic_enabled: bool,
) {
    let pending = PendingSettingsProjectConfig {
        path,
        hashline_edit,
        compaction_enabled,
        compaction_auto_retry_overflow,
        compaction_structured_summary_contract,
        compaction_estimated_token_triggers,
        deterministic_enabled,
    };
    #[cfg(test)]
    {
        PENDING_SETTINGS_PROJECT_CONFIG.with(|slot| {
            *slot.borrow_mut() = Some(pending);
        });
    }
    #[cfg(not(test))]
    {
        *PENDING_SETTINGS_PROJECT_CONFIG.lock().unwrap_or_abort() = Some(pending);
    }
}

pub(super) fn take_pending_settings_project_config() -> Option<PendingSettingsProjectConfig> {
    #[cfg(test)]
    {
        PENDING_SETTINGS_PROJECT_CONFIG.with(|slot| slot.borrow_mut().take())
    }
    #[cfg(not(test))]
    {
        PENDING_SETTINGS_PROJECT_CONFIG
            .lock()
            .unwrap_or_abort()
            .take()
    }
}

pub fn set_pending_connect_providers(providers: Vec<ConnectProviderOption>) {
    #[cfg(not(test))]
    {
        *PENDING_CONNECT_PROVIDERS.lock().unwrap_or_abort() = providers;
    }
    #[cfg(test)]
    {
        PENDING_CONNECT_PROVIDERS.with(|slot| {
            *slot.borrow_mut() = providers;
        });
    }
}

pub(super) fn take_pending_connect_providers() -> Vec<ConnectProviderOption> {
    #[cfg(not(test))]
    {
        PENDING_CONNECT_PROVIDERS.lock().unwrap_or_abort().clone()
    }
    #[cfg(test)]
    {
        PENDING_CONNECT_PROVIDERS.with(|slot| slot.borrow().clone())
    }
}

pub(super) fn take_pending_live_launch_metadata() -> Option<LaunchMetadata> {
    PendingLiveState::take_launch_metadata()
}

pub fn set_pending_live_prompt_draft(draft: Option<String>) {
    PendingLiveState::set_prompt(non_empty_prompt(draft), false);
}

pub fn set_pending_live_prompt_auto_submit(prompt: Option<String>) {
    let prompt = non_empty_prompt(prompt);
    let should_auto_submit = prompt.is_some();
    PendingLiveState::set_prompt(prompt, should_auto_submit);
}

pub(super) fn take_pending_live_prompt() -> Option<PendingLivePrompt> {
    PendingLiveState::take_prompt()
}

fn non_empty_prompt(prompt: Option<String>) -> Option<String> {
    prompt.filter(|value| has_trimmed_content(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use harness_core::auth::plugin::AuthMethodSpec;
    use harness_core::auth::ProviderId;

    fn provider() -> ConnectProviderOption {
        ConnectProviderOption {
            id: ProviderId::parse("openai").unwrap_or_abort(),
            label: "OpenAI".to_string(),
            description: "API key".to_string(),
            methods: vec![AuthMethodSpec::ApiKey {
                label: "API key".to_string(),
            }],
            models: Vec::new(),
        }
    }

    #[test]
    fn connect_providers_remain_available_across_app_state_transitions() {
        // arrange
        set_pending_connect_providers(vec![provider()]);

        // act
        let startup = AppState::new_startup(Vec::new(), None);
        let live = AppState::new_live(None, false, None);

        // assert
        assert_eq!(startup.connect_dialog.providers.len(), 1);
        assert_eq!(live.connect_dialog.providers.len(), 1);
    }
}

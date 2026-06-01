use ratatui::text::{Line, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingStep {
    StartSplash,
    ProviderPick,
    AuthMethodPick,
    CopilotTargetPick,
    CodexBrowser,
    CodexDevice,
    CopilotPublicDevice,
    CopilotEnterpriseDevice,
    ApiKeyEntry,
    LoginSuccess,
    LoginErrorTimeout,
    SkipConfirmation,
    FirstPromptSuccess,
    SkillSelection,
}

impl OnboardingStep {
    pub const INVENTORY: [Self; 14] = [
        Self::StartSplash,
        Self::ProviderPick,
        Self::AuthMethodPick,
        Self::CopilotTargetPick,
        Self::CodexBrowser,
        Self::CodexDevice,
        Self::CopilotPublicDevice,
        Self::CopilotEnterpriseDevice,
        Self::ApiKeyEntry,
        Self::LoginSuccess,
        Self::LoginErrorTimeout,
        Self::SkipConfirmation,
        Self::FirstPromptSuccess,
        Self::SkillSelection,
    ];

    pub fn next(self) -> Self {
        match self {
            Self::StartSplash => Self::ProviderPick,
            Self::ProviderPick => Self::AuthMethodPick,
            Self::AuthMethodPick => Self::CodexDevice,
            Self::CopilotTargetPick => Self::CopilotPublicDevice,
            Self::CodexBrowser => Self::LoginSuccess,
            Self::CodexDevice => Self::LoginSuccess,
            Self::CopilotPublicDevice => Self::LoginSuccess,
            Self::CopilotEnterpriseDevice => Self::LoginSuccess,
            Self::ApiKeyEntry => Self::LoginSuccess,
            Self::LoginSuccess => Self::SkillSelection,
            Self::LoginErrorTimeout => Self::AuthMethodPick,
            Self::SkipConfirmation => Self::FirstPromptSuccess,
            Self::FirstPromptSuccess => Self::FirstPromptSuccess,
            Self::SkillSelection => Self::FirstPromptSuccess,
        }
    }

    pub fn snapshot_name(self) -> &'static str {
        match self {
            Self::StartSplash => "start_splash",
            Self::ProviderPick => "provider_pick",
            Self::AuthMethodPick => "auth_method_pick",
            Self::CopilotTargetPick => "copilot_target_pick",
            Self::CodexBrowser => "codex_browser",
            Self::CodexDevice => "codex_device",
            Self::CopilotPublicDevice => "copilot_public_device",
            Self::CopilotEnterpriseDevice => "copilot_enterprise_device",
            Self::ApiKeyEntry => "api_key_entry",
            Self::LoginSuccess => "login_success",
            Self::LoginErrorTimeout => "login_error_timeout",
            Self::SkipConfirmation => "skip_confirmation",
            Self::FirstPromptSuccess => "first_prompt_success",
            Self::SkillSelection => "skill_selection",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardingChoice {
    pub label: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardingScreen {
    pub step: OnboardingStep,
    pub eyebrow: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    pub choices: Vec<OnboardingChoice>,
    pub selected: usize,
    pub status: Option<&'static str>,
    pub footer: &'static str,
}

pub fn screen_for(step: OnboardingStep, selected: usize) -> OnboardingScreen {
    let (eyebrow, title, body, choices, status, footer) = match step {
        OnboardingStep::StartSplash => (
            "HARNESS ONBOARDING",
            "Connect a provider",
            "Harness can use stored OAuth, stored API keys, apiKeyEnv, or inline config. This launch can also be skipped.",
            vec![
                choice("Start setup", "Choose provider and auth method"),
                choice("Skip this launch", "Use existing config only; no credential is written"),
            ],
            None,
            "↑↓ select · enter choose · esc skip · ctrl+p commands",
        ),
        OnboardingStep::ProviderPick => (
            "PROVIDER",
            "Select provider",
            "Only V1 built-in auth provider ids are shown.",
            vec![
                choice("Codex", "Browser, device-code, or stored API key"),
                choice("GitHub Copilot", "Public or Enterprise device-code login"),
            ],
            None,
            "↑↓ select · enter continue · esc back",
        ),
        OnboardingStep::AuthMethodPick => (
            "AUTH METHOD",
            "Choose login method",
            "Methods are provider-declared; unavailable methods are not listed.",
            vec![
                choice("Device code", "Copy a code into the provider page"),
                choice("Browser", "Open Codex OAuth in a browser"),
                choice("API key", "Read once from stdin and store securely"),
            ],
            None,
            "↑↓ select · enter login · esc provider",
        ),
        OnboardingStep::CopilotTargetPick => (
            "COPILOT TARGET",
            "Choose Copilot target",
            "GitHub Copilot supports public GitHub.com and Enterprise device-code login in V1.",
            vec![
                choice("GitHub.com", "Public Copilot API base"),
                choice("Enterprise", "Enter Enterprise domain before device login"),
            ],
            None,
            "↑↓ select · enter continue · esc provider",
        ),
        OnboardingStep::CodexBrowser => (
            "CODEX BROWSER",
            "Authorize Codex in browser",
            "Harness opens a PKCE URL and waits for loopback callback. Codes and tokens are redacted from status output.",
            vec![choice("Open browser", "Use PKCE loopback on localhost")],
            Some("Waiting for callback · account <redacted>"),
            "enter open · esc cancel",
        ),
        OnboardingStep::CodexDevice => (
            "CODEX DEVICE",
            "Authorize Codex by device code",
            "Open the verification URL and enter the displayed code. Polling handles pending and timeout states.",
            vec![choice("I entered the code", "Continue polling for completion")],
            Some("Verification code <redacted> · device id <redacted>"),
            "enter continue · esc cancel",
        ),
        OnboardingStep::CopilotPublicDevice => (
            "COPILOT PUBLIC",
            "Authorize GitHub Copilot",
            "Use github.com device login. Harness stores the resulting bearer in the secure credential store.",
            vec![choice("Use GitHub.com", "Public Copilot API base")],
            Some("User code <redacted>"),
            "enter continue · esc cancel",
        ),
        OnboardingStep::CopilotEnterpriseDevice => (
            "COPILOT ENTERPRISE",
            "Authorize Enterprise Copilot",
            "Enter and validate the Enterprise domain, then use the Enterprise device-code flow.",
            vec![choice("Use Enterprise", "copilot-api.<enterprise-domain>")],
            Some("Enterprise domain <redacted>"),
            "enter continue · esc cancel",
        ),
        OnboardingStep::ApiKeyEntry => (
            "API KEY",
            "Store API key securely",
            "Paste the key into the protected prompt. The key is never echoed, logged, or written to config.",
            vec![choice("Save key", "Stored as api_key credential")],
            Some("Input hidden · secret redacted"),
            "enter save · esc cancel",
        ),
        OnboardingStep::LoginSuccess => (
            "SUCCESS",
            "Credential stored",
            "Provider auth is ready without a live transport probe. You can relogin or logout later from /auth.",
            vec![choice("Continue", "Move to first prompt")],
            Some("Stored credential present · secret redacted"),
            "enter continue",
        ),
        OnboardingStep::LoginErrorTimeout => (
            "LOGIN ERROR",
            "Authorization timed out",
            "No credential was stored. Retry, choose another method, or skip this launch.",
            vec![
                choice("Retry", "Return to auth method selection"),
                choice("Skip this launch", "Do not write credentials"),
            ],
            Some("Timeout · no secret stored"),
            "↑↓ select · enter choose · esc skip",
        ),
        OnboardingStep::SkipConfirmation => (
            "SKIP",
            "Skip onboarding for this launch?",
            "Skipping writes no persistent flag and does not create, replace, or delete credentials.",
            vec![
                choice("Skip once", "Use configured provider path now"),
                choice("Back", "Return to setup"),
            ],
            None,
            "↑↓ select · enter choose · esc back",
        ),
        OnboardingStep::FirstPromptSuccess => (
            "FIRST PROMPT",
            "Ready for first prompt",
            "Send a prompt to confirm the selected provider path. Success appears in the live transcript.",
            vec![choice("Start session", "Open the transcript-first shell")],
            Some("First prompt success signal ready"),
            "enter start · ctrl+p commands",
        ),
        OnboardingStep::SkillSelection => (
            "SKILLS",
            "Select skills",
            "Skill rows use the same grouped list, name, description, and selection behavior as command surfaces.",
            vec![
                choice("build", "Implementation and verification workflow"),
                choice("plan", "Read-only planning lane"),
                choice("explore", "Fast repository lookup"),
            ],
            Some("3 skills available · bundled resources load only on activation"),
            "↑↓ select · space toggle · enter continue",
        ),
    };

    OnboardingScreen {
        step,
        eyebrow,
        title,
        body,
        selected: selected.min(choices.len().saturating_sub(1)),
        choices,
        status,
        footer,
    }
}

fn choice(label: &'static str, description: &'static str) -> OnboardingChoice {
    OnboardingChoice { label, description }
}

impl OnboardingScreen {
    pub fn lines(&self) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::from(Span::raw(self.eyebrow)),
            Line::from(Span::raw(self.title)),
            Line::from(Span::raw(self.body)),
            Line::from(Span::raw("")),
        ];
        for (index, choice) in self.choices.iter().enumerate() {
            let marker = if index == self.selected { "●" } else { "○" };
            lines.push(Line::from(vec![
                Span::raw(format!("{marker} {}", choice.label)),
                Span::raw(format!(" — {}", choice.description)),
            ]));
        }
        if let Some(status) = self.status {
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::raw(status)));
        }
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::raw(self.footer)));
        lines
    }
}

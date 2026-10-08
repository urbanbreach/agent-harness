//! Operator guidance.
use super::*;

// ---- guidance ----

pub fn no_account_guidance(has_anthropic_credential: bool) -> String {
    let mut lines = vec![
        format!("No Claude account configured for {PROVIDER}."),
        format!("  /login {PROVIDER}  - sign in with your Claude Pro/Max subscription"),
    ];
    if has_anthropic_credential {
        lines.push("  (your existing Anthropic OAuth login will be offered as an import)".into());
    }
    lines.extend([
        "  Or set CLAUDE_CODE_OAUTH_TOKEN (and _2.._N for more accounts),".into(),
        "  or log in with the claude CLI for ambient auth.".into(),
    ]);
    lines.join("\n")
}

pub fn all_accounts_blocked_guidance(soonest: Option<i64>, auth_error: bool) -> String {
    if auth_error {
        return [
            format!(
                "All Claude accounts for {PROVIDER} are currently blocked (authentication error)."
            ),
            format!("  /login {PROVIDER}  - re-authenticate to refresh the blocked account"),
            "  /claude-account list  - inspect account states".into(),
        ]
        .join("\n");
    }
    let eta = soonest.map_or_else(|| "after re-login".into(), iso);
    [
        format!(
            "All Claude accounts for {PROVIDER} are currently blocked (rate limit or auth errors)."
        ),
        format!("  Soonest automatic retry: {eta}."),
        "  /claude-account list  - inspect account states".into(),
        format!("  /login {PROVIDER}  - add another account"),
    ]
    .join("\n")
}

/// The remedy for a version-floor 400 depends on which binary ran.
fn version_floor_remedy(target: &str, ran: Option<&ClaudeCodeRun>) -> String {
    match ran.map(|run| (run.source, run.executable.display().to_string())) {
        Some((ExecutableSource::Override, executable)) => format!(
            "The Claude Code binary set by CLAUDE_CODE_EXECUTABLE ({executable}) is too old for this model. Replace it with {target}, or unset CLAUDE_CODE_EXECUTABLE to use the installed `claude`."
        ),
        Some((ExecutableSource::Installed, executable)) => format!(
            "The installed Claude Code ({executable}) is too old for this model. Run `claude update` to get {target}, or set CLAUDE_CODE_EXECUTABLE to {target} binary."
        ),
        Some((ExecutableSource::Path, executable)) => format!(
            "The Claude Code on PATH ({executable}) is too old for this model. Run `claude update` to get {target}, or set CLAUDE_CODE_EXECUTABLE to {target} binary."
        ),
        None => format!(
            "The Claude Code binary is too old for this model. Update the harness, update the `claude` on PATH, or set CLAUDE_CODE_EXECUTABLE to {target} binary."
        ),
    }
}

static VERSION_FLOOR: LazyLock<Option<Regex>> = LazyLock::new(|| {
    re(
        r"(?i)does not support this model; version (\S+?) or newer is required|claude_code_version_too_old",
    )
});
static UNKNOWN_MODEL: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"(?i)\bmodel_not_found\b|unrecognized_model|not found for provider"));

pub fn claude_code_version_floor_guidance(
    text: &str,
    ran: Option<&ClaudeCodeRun>,
) -> Option<String> {
    if let Some(captures) = VERSION_FLOOR.as_ref().and_then(|re| re.captures(text)) {
        let target = captures.get(1).map_or_else(
            || "a newer Claude Code".to_owned(),
            |v| {
                format!(
                    "Claude Code {} or newer",
                    v.as_str().trim_end_matches(['.', ',', ';', ':'])
                )
            },
        );
        return Some(version_floor_remedy(&target, ran));
    }
    matches(&UNKNOWN_MODEL, text).then(|| {
        "The Claude Code binary the harness runs does not know this model id; update the harness or set CLAUDE_CODE_EXECUTABLE to a newer Claude Code binary.".into()
    })
}

pub fn sdk_error_guidance(kind: SdkErrorKind) -> Option<String> {
    Some(match kind {
        SdkErrorKind::OrgNotAllowed => "This organization's policy disallows subscription OAuth use here. Use an API key (ANTHROPIC_API_KEY) or an account from an allowed organization.".into(),
        SdkErrorKind::Billing => "The selected Claude account has a billing problem. Check the plan at claude.com or switch accounts with /claude-account pin <name>.".into(),
        SdkErrorKind::AuthError => format!("The account's OAuth token was rejected. Re-run /login {PROVIDER} to refresh it, or remove the account with /claude-account remove <name>."),
        SdkErrorKind::Entitlement => "This model needs usage credits on the selected Claude account (it is not included in the subscription). Switch models with /model, enable usage credits at claude.com, or pick another account with /claude-account pin <name>.".into(),
        _ => return None,
    })
}

pub fn override_system_prompt_guidance(path: Option<&str>, reason: &str) -> String {
    let target = path.map_or_else(
        || "systemPromptFile".to_owned(),
        |path| format!("systemPromptFile \"{path}\""),
    );
    format!(
        "Anthropic Subscription override prompt could not load {target}: {reason}. Set anthropic_subscription.system_prompt_file to a readable, non-empty UTF-8 prompt file, or select system_prompt_mode \"full\"."
    )
}

static ARMED_SESSIONS: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

pub fn preset_append_deprecation_guidance(
    preset_append: bool,
    conflict: bool,
    session_id: &str,
) -> Option<String> {
    let mut armed = ARMED_SESSIONS.lock().ok()?;
    if armed.contains(session_id) || !conflict && !preset_append {
        return None;
    }
    armed.insert(session_id.into());
    let mut parts = Vec::new();
    if preset_append {
        parts.push("preset-append system-prompt mode is deprecated; `full` mode delivers the complete harness system prompt; preset-append will be removed after one release.");
    }
    if conflict {
        parts.push("systemPromptMode wins.");
    }
    Some(parts.join(" "))
}

/// `withAuthGuidance`: the error text plus every hint that applies.
pub fn with_auth_guidance(error: &LaneError, message: &str, ran: Option<&ClaudeCodeRun>) -> String {
    let blocked = match error {
        LaneError::AllAccountsBlocked(e) => Some(e),
        LaneError::Classified { original, .. } => match original.as_ref() {
            LaneError::AllAccountsBlocked(e) => Some(e),
            _ => None,
        },
        _ => None,
    };
    if let Some(blocked) = blocked {
        return all_accounts_blocked_guidance(blocked.soonest_unblock_at, blocked.auth_error);
    }
    let hints: Vec<String> = [
        sdk_error_guidance(classify_lane_error(error).kind),
        claude_code_version_floor_guidance(message, ran),
    ]
    .into_iter()
    .flatten()
    .collect();
    if hints.is_empty() {
        message.to_owned()
    } else {
        format!("{message}\n{}", hints.join("\n"))
    }
}

use super::*;

pub(crate) fn composer_model_badge(
    app: &AppState,
    extra_identity: &[String],
    max_width: usize,
) -> String {
    let model = app.current_model_base_label();
    let mut identity = Vec::new();
    if !model.is_empty() && model != "-" && !model.eq_ignore_ascii_case("unknown") {
        identity.push(model.to_string());
    } else if !app.composer.prompt_buffer.is_empty() {
        identity.push("unknown".to_string());
    }
    if let Some(reasoning) = app.current_model_reasoning_label() {
        if !reasoning.is_empty()
            && !identity
                .iter()
                .any(|part| part.eq_ignore_ascii_case(reasoning) || part.contains(reasoning))
        {
            identity.push(reasoning.to_string());
        }
    }
    identity.extend(extra_identity.iter().cloned());

    let mut status = Vec::new();
    if app.always_approve_mode() {
        status.push("always-approve".to_string());
    }
    if app.shell_mode() {
        status.push("shell".to_string());
    }
    if app.queued_prompt_count > 0 {
        status.push(format!("queued {}", app.queued_prompt_count));
    }
    if app.composer.multiline_mode {
        status.push("multiline".to_string());
    }

    let identity = identity.join(" · ");
    let status = status.join(" · ");
    if identity.is_empty() {
        return truncate_plain_text(&status, max_width);
    }
    if status.is_empty() {
        return truncate_plain_text(&identity, max_width);
    }

    let joined = format!("{identity} · {status}");
    if display_width(&joined) <= max_width {
        return joined;
    }

    let status_width = display_width(&status);
    if status_width >= max_width {
        return truncate_plain_text(&status, max_width);
    }
    let separator = " · ";
    let identity_width = max_width.saturating_sub(status_width + display_width(separator));
    let identity = truncate_plain_text(&identity, identity_width);
    format!("{identity}{separator}{status}")
}

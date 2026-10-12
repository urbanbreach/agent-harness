use crate::config::{EditConfig, EditMode};

/// Resolve the model preference, then preserve the agent's available editing tool.
pub fn resolve_edit_mode(config: &EditConfig, model: &str, edit: bool, patch: bool) -> EditMode {
    let model = model.to_ascii_lowercase();
    let selected = config
        .model_variants
        .iter()
        .filter(|(key, _)| {
            key.is_empty()
                || model
                    .as_bytes()
                    .windows(key.len())
                    .any(|part| part.eq_ignore_ascii_case(key.as_bytes()))
        })
        .max_by(|(left, _), (right, _)| left.len().cmp(&right.len()).then_with(|| right.cmp(left)))
        .map_or(config.mode, |(_, mode)| *mode);
    let mode = if selected == EditMode::Auto {
        auto_mode(&model)
    } else {
        selected
    };
    match mode {
        EditMode::ApplyPatch if !patch && edit => EditMode::Replace,
        EditMode::Hashline | EditMode::Replace if !edit && patch => EditMode::ApplyPatch,
        _ => mode,
    }
}

fn auto_mode(model: &str) -> EditMode {
    let openai = model.split(['/', '@', ':', '.', '_', '-']).any(|part| {
        part == "gpt"
            || part
                .strip_prefix("gpt")
                .is_some_and(|tail| tail.starts_with(|c: char| c.is_ascii_digit()))
            || part == "codex"
            || part
                .strip_prefix('o')
                .is_some_and(|tail| !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()))
    });
    if openai {
        EditMode::ApplyPatch
    } else if ["kimi", "mimo", "minimax", "deepseek", "stepfun"]
        .iter()
        .any(|family| model.contains(family))
        || (model.contains("glm") && model.contains("flash"))
    {
        EditMode::Replace
    } else {
        EditMode::Hashline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_precedence_and_tool_availability() {
        let cases = [
            (
                EditMode::Auto,
                "codex/gpt-6-astra",
                true,
                true,
                EditMode::ApplyPatch,
            ),
            (
                EditMode::Auto,
                "openai-codex/gpt-6-luna",
                true,
                true,
                EditMode::ApplyPatch,
            ),
            (
                EditMode::Auto,
                "openai:o3-mini",
                true,
                true,
                EditMode::ApplyPatch,
            ),
            (EditMode::Auto, "xgpt-proxy", true, true, EditMode::Hashline),
            (
                EditMode::Auto,
                "x codexical",
                true,
                true,
                EditMode::Hashline,
            ),
            (EditMode::Auto, "kimi-k2", true, true, EditMode::Replace),
            (EditMode::Auto, "MiMo-v2", true, true, EditMode::Replace),
            (EditMode::Auto, "MiniMax-m2", true, true, EditMode::Replace),
            (EditMode::Auto, "DeepSeek-v4", true, true, EditMode::Replace),
            (
                EditMode::Auto,
                "stepfun/step-3",
                true,
                true,
                EditMode::Replace,
            ),
            (EditMode::Auto, "glm-5-flash", true, true, EditMode::Replace),
            (EditMode::Replace, "gpt-6", true, true, EditMode::Replace),
            (EditMode::Hashline, "gpt-6", true, true, EditMode::Hashline),
            (
                EditMode::ApplyPatch,
                "claude",
                true,
                false,
                EditMode::Replace,
            ),
            (
                EditMode::Hashline,
                "claude",
                false,
                true,
                EditMode::ApplyPatch,
            ),
            (
                EditMode::Replace,
                "claude",
                false,
                true,
                EditMode::ApplyPatch,
            ),
            (
                EditMode::Hashline,
                "claude",
                false,
                false,
                EditMode::Hashline,
            ),
        ];
        for (mode, model, edit, patch, expected) in cases {
            let config = EditConfig {
                mode,
                ..Default::default()
            };
            assert_eq!(
                resolve_edit_mode(&config, model, edit, patch),
                expected,
                "{model}"
            );
        }
        let config = EditConfig {
            mode: EditMode::ApplyPatch,
            model_variants: [
                ("GPT".into(), EditMode::Replace),
                ("gpt-6".into(), EditMode::Hashline),
            ]
            .into(),
        };
        assert_eq!(
            resolve_edit_mode(&config, "OPENAI:GPT-6", true, true),
            EditMode::Hashline
        );
    }
}

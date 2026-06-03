use std::path::{Component, Path};

use crate::text::non_empty_trimmed;

use super::*;

pub(super) fn is_blank_config_value(value: &str) -> bool {
    non_empty_trimmed(value).is_none()
}

pub(super) fn validate_mcp_servers(config: &HarnessConfig) -> Result<(), ConfigError> {
    for (server_name, server) in &config.integrations.mcp.servers {
        if is_blank_config_value(server_name) {
            return Err(ConfigError::InvalidReference(
                "integrations.mcp.servers contains an empty server name; use explicit ids like `docs-rs` or `gh_grep`"
                    .to_string(),
            ));
        }
        if !server_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        {
            return Err(ConfigError::InvalidReference(format!(
                "integrations.mcp.servers.{server_name} has an invalid server id; use only ASCII letters, digits, `_`, or `-`"
            )));
        }

        match server {
            McpServerConfig::Stdio {
                command,
                env,
                cwd,
                timeout_secs,
                ..
            } => {
                if command.is_empty() {
                    return Err(ConfigError::InvalidReference(format!(
                        "integrations.mcp.servers.{server_name} must include at least one stdio command token"
                    )));
                }
                if command.iter().any(|token| is_blank_config_value(token)) {
                    return Err(ConfigError::InvalidReference(format!(
                        "integrations.mcp.servers.{server_name} contains an empty stdio command token"
                    )));
                }
                if *timeout_secs == 0 {
                    return Err(ConfigError::InvalidReference(format!(
                        "integrations.mcp.servers.{server_name} must set `timeout_secs` to a value greater than 0"
                    )));
                }
                if let Some(cwd) = cwd {
                    if cwd.as_os_str().is_empty() {
                        return Err(ConfigError::InvalidReference(format!(
                            "integrations.mcp.servers.{server_name} must set `cwd` to a non-empty path when provided"
                        )));
                    }
                }
                for key in env.keys() {
                    if is_blank_config_value(key) {
                        return Err(ConfigError::InvalidReference(format!(
                            "integrations.mcp.servers.{server_name} contains an empty environment variable name"
                        )));
                    }
                }
            }
            McpServerConfig::Http {
                endpoint,
                headers,
                timeout_secs,
                ..
            } => {
                if is_blank_config_value(endpoint) {
                    return Err(ConfigError::InvalidReference(format!(
                        "integrations.mcp.servers.{server_name} must set a non-empty HTTP endpoint"
                    )));
                }
                if *timeout_secs == 0 {
                    return Err(ConfigError::InvalidReference(format!(
                        "integrations.mcp.servers.{server_name} must set `timeout_secs` to a value greater than 0"
                    )));
                }
                for key in headers.keys() {
                    if is_blank_config_value(key) {
                        return Err(ConfigError::InvalidReference(format!(
                            "integrations.mcp.servers.{server_name} contains an empty HTTP header name"
                        )));
                    }
                }
            }
        }
    }

    Ok(())
}

pub(super) fn validate_hook_definitions(config: &HarnessConfig) -> Result<(), ConfigError> {
    for (index, hook) in config.hooks.lifecycle.iter().enumerate() {
        if hook.id.as_deref().is_some_and(is_blank_config_value) {
            return Err(ConfigError::InvalidReference(format!(
                "hooks.lifecycle[{index}] for event `{}` must set a non-empty `id` when provided",
                hook.event.as_str()
            )));
        }

        if hook.command.is_empty() {
            return Err(ConfigError::InvalidReference(format!(
                "hooks.lifecycle[{index}] for event `{}` must include at least one command token",
                hook.event.as_str()
            )));
        }

        if hook
            .command
            .iter()
            .any(|token| is_blank_config_value(token))
        {
            return Err(ConfigError::InvalidReference(format!(
                "hooks.lifecycle[{index}] for event `{}` contains an empty command token; remove blank values",
                hook.event.as_str()
            )));
        }

        if hook.timeout_ms == 0 {
            return Err(ConfigError::InvalidReference(format!(
                "hooks.lifecycle[{index}] for event `{}` must set `timeout_ms` to a value greater than 0",
                hook.event.as_str()
            )));
        }

        if let Some(cwd) = hook.cwd.as_deref() {
            let cwd_path = Path::new(cwd);
            if is_blank_config_value(cwd) {
                return Err(ConfigError::InvalidReference(format!(
                    "hooks.lifecycle[{index}] for event `{}` must set `cwd` to a non-empty relative path when provided",
                    hook.event.as_str()
                )));
            }

            if cwd_path.is_absolute()
                || cwd_path
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
            {
                return Err(ConfigError::InvalidReference(format!(
                    "hooks.lifecycle[{index}] for event `{}` has invalid `cwd` `{cwd}`; use a workspace-relative path without `..`",
                    hook.event.as_str()
                )));
            }
        }

        for key in hook.env.keys() {
            if is_blank_config_value(key) {
                return Err(ConfigError::InvalidReference(format!(
                    "hooks.lifecycle[{index}] for event `{}` contains an empty environment variable name",
                    hook.event.as_str()
                )));
            }
        }
    }

    Ok(())
}

pub(super) fn validate_skill_roots(config: &HarnessConfig) -> Result<(), ConfigError> {
    for (index, root) in config.skills.project_roots.iter().enumerate() {
        validate_skill_root(root, &format!("skills.project_roots[{index}]"))?;
    }

    for (index, root) in config.skills.global_roots.iter().enumerate() {
        validate_skill_root(root, &format!("skills.global_roots[{index}]"))?;
    }

    for pattern in config.skills.permissions.keys() {
        if is_blank_config_value(pattern) {
            return Err(ConfigError::InvalidReference(
                "skills.permissions contains an empty pattern key; use explicit patterns like `*` or `internal-*`"
                    .to_string(),
            ));
        }
    }

    Ok(())
}

pub(super) fn validate_lsp_overrides(config: &HarnessConfig) -> Result<(), ConfigError> {
    for (server_name, server) in &config.lsp.servers {
        if is_blank_config_value(server_name) {
            return Err(ConfigError::InvalidReference(
                "lsp.servers contains an empty server key; use a stable id like `rust` or `typescript`"
                    .to_string(),
            ));
        }

        if !is_builtin_lsp_server(server_name) && server.command.is_none() {
            return Err(ConfigError::InvalidReference(format!(
                "lsp.servers.`{server_name}` must provide `command` for custom local servers"
            )));
        }

        if !is_builtin_lsp_server(server_name) && server.extensions.is_none() {
            return Err(ConfigError::InvalidReference(format!(
                "lsp.servers.`{server_name}` must provide `extensions` for custom local servers"
            )));
        }

        if let Some(command) = server.command.as_ref() {
            if command.is_empty() {
                return Err(ConfigError::InvalidReference(format!(
                    "lsp.servers.`{server_name}` must provide at least one command token when `command` is set"
                )));
            }

            if command.iter().any(|token| is_blank_config_value(token)) {
                return Err(ConfigError::InvalidReference(format!(
                    "lsp.servers.`{server_name}` contains an empty command token; remove blank values"
                )));
            }
        }

        if let Some(extensions) = server.extensions.as_ref() {
            if extensions.is_empty() {
                return Err(ConfigError::InvalidReference(format!(
                    "lsp.servers.`{server_name}` must include at least one extension when `extensions` is set"
                )));
            }

            for extension in extensions {
                if !(extension.starts_with('.') && extension.len() > 1) {
                    return Err(ConfigError::InvalidReference(format!(
                        "lsp.servers.`{server_name}` has invalid extension `{extension}`; expected values like `.rs` or `.tsx`"
                    )));
                }
            }
        }

        for key in server.env.keys() {
            if is_blank_config_value(key) {
                return Err(ConfigError::InvalidReference(format!(
                    "lsp.servers.`{server_name}` contains an empty `env` key; use non-empty environment variable names"
                )));
            }
        }

        if let Some(initialization) = server.initialization.as_ref() {
            if !initialization.is_object() {
                return Err(ConfigError::InvalidReference(format!(
                    "lsp.servers.`{server_name}` `initialization` must be a JSON object"
                )));
            }
        }
    }

    Ok(())
}

fn validate_skill_root(root: &Path, location: &str) -> Result<(), ConfigError> {
    if root.as_os_str().is_empty() {
        return Err(ConfigError::InvalidReference(format!(
            "{location} must not be empty; use a normalized relative or absolute skill root"
        )));
    }

    if root
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(ConfigError::InvalidReference(format!(
            "{location} `{}` must not contain `..`; use a normalized skill root path",
            root.display()
        )));
    }

    Ok(())
}

fn is_builtin_lsp_server(name: &str) -> bool {
    matches!(
        name,
        "go" | "json" | "python" | "rust" | "typescript" | "yaml"
    )
}

//! OpenCode-parity formatter discovery rules.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::{
    find_up::{
        find_up, read_json_file, read_text_file, resolve_npm_binary, run_command_check,
        which_binary,
    },
    registry::{DiscoveryKind, BUILTIN_FORMATTERS},
    DiscoveryContext, FormatterDiscovery,
};

mod support;
pub(crate) use support::{
    first_line, has_dep, nearest_marker_dir, text_marker_contains, ComposerJson, PackageJson,
};

/// Real OpenCode-parity formatter discovery.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFormatterDiscovery;

#[async_trait]
impl FormatterDiscovery for RealFormatterDiscovery {
    async fn resolve(&self, name: &str, context: &DiscoveryContext) -> Option<Vec<String>> {
        let info = BUILTIN_FORMATTERS.iter().find(|info| info.name == name)?;

        match info.discovery_kind {
            DiscoveryKind::WhichOnly => {
                let path = which_binary(name).await?;
                let mut command: Vec<String> =
                    info.command.iter().map(|&arg| arg.to_string()).collect();
                if command.is_empty() {
                    return None;
                }
                command[0] = path.to_string_lossy().to_string();
                Some(command)
            }
            DiscoveryKind::Prettier => resolve_prettier(context).await,
            DiscoveryKind::Biome => resolve_biome(context).await,
            DiscoveryKind::Oxfmt => resolve_oxfmt(context).await,
            DiscoveryKind::ClangFormat => resolve_clang_format(context).await,
            DiscoveryKind::Ruff => resolve_ruff(context).await,
            DiscoveryKind::UvFormat => resolve_uv_format(context).await,
            DiscoveryKind::Ocamlformat => resolve_ocaml_format(context).await,
            DiscoveryKind::Pint => resolve_pint(context).await,
            DiscoveryKind::Air => resolve_air(context).await,
        }
    }
}

async fn resolve_prettier(context: &DiscoveryContext) -> Option<Vec<String>> {
    for package_json in find_up("package.json", &context.target_dir, &context.workspace_root).await
    {
        let Some(package): Option<PackageJson> = read_json_file(&package_json).await else {
            continue;
        };
        if !has_dep(package.dependencies.as_ref(), "prettier")
            && !has_dep(package.dev_dependencies.as_ref(), "prettier")
        {
            continue;
        }
        let marker_dir = package_json.parent()?.to_path_buf();
        let binary = resolve_npm_binary("prettier", &marker_dir).await?;
        return Some(vec![
            binary.to_string_lossy().to_string(),
            "--write".to_string(),
            "$FILE".to_string(),
        ]);
    }
    None
}

async fn resolve_biome(context: &DiscoveryContext) -> Option<Vec<String>> {
    let marker_dir = nearest_marker_dir(
        &["biome.json", "biome.jsonc"],
        &context.target_dir,
        &context.workspace_root,
    )
    .await?;
    let binary = resolve_npm_binary("biome", &marker_dir).await?;
    Some(vec![
        binary.to_string_lossy().to_string(),
        "format".into(),
        "--write".into(),
        "$FILE".into(),
    ])
}

async fn resolve_oxfmt(context: &DiscoveryContext) -> Option<Vec<String>> {
    if !context.experimental_oxfmt {
        return None;
    }
    for package_json in find_up("package.json", &context.target_dir, &context.workspace_root).await
    {
        let Some(package): Option<PackageJson> = read_json_file(&package_json).await else {
            continue;
        };
        if !has_dep(package.dependencies.as_ref(), "oxfmt")
            && !has_dep(package.dev_dependencies.as_ref(), "oxfmt")
        {
            continue;
        }
        let marker_dir = package_json.parent()?.to_path_buf();
        let binary = resolve_npm_binary("oxfmt", &marker_dir).await?;
        return Some(vec![
            binary.to_string_lossy().to_string(),
            "$FILE".to_string(),
        ]);
    }
    None
}

async fn resolve_clang_format(context: &DiscoveryContext) -> Option<Vec<String>> {
    let _config = find_up(
        ".clang-format",
        &context.target_dir,
        &context.workspace_root,
    )
    .await
    .into_iter()
    .next()?;
    let path = which_binary("clang-format").await?;
    Some(vec![
        path.to_string_lossy().to_string(),
        "-i".into(),
        "$FILE".into(),
    ])
}

async fn resolve_ruff(context: &DiscoveryContext) -> Option<Vec<String>> {
    which_binary("ruff").await?;

    if ruff_config_dir(context).await.is_some() || ruff_fallback(context).await {
        return Some(vec!["ruff".into(), "format".into(), "$FILE".into()]);
    }
    None
}

async fn ruff_config_dir(context: &DiscoveryContext) -> Option<PathBuf> {
    for name in ["pyproject.toml", "ruff.toml", ".ruff.toml"] {
        let Some(marker) = find_up(name, &context.target_dir, &context.workspace_root)
            .await
            .into_iter()
            .next()
        else {
            continue;
        };
        if name == "pyproject.toml" {
            let text = read_text_file(&marker).await?;
            if !text.contains("[tool.ruff]") {
                continue;
            }
        }
        return marker.parent().map(Path::to_path_buf);
    }
    None
}

async fn ruff_fallback(context: &DiscoveryContext) -> bool {
    text_marker_contains(
        &["requirements.txt", "pyproject.toml", "Pipfile"],
        "ruff",
        &context.target_dir,
        &context.workspace_root,
    )
    .await
}

async fn resolve_uv_format(context: &DiscoveryContext) -> Option<Vec<String>> {
    if resolve_ruff(context).await.is_some() {
        return None;
    }
    let uv = which_binary("uv").await?;
    let uv_path = uv.to_string_lossy().to_string();
    let check = run_command_check(&uv_path, &["format", "--help"]).await?;
    if check.exit_code == 0 {
        Some(vec![uv_path, "format".into(), "--".into(), "$FILE".into()])
    } else {
        None
    }
}

async fn resolve_ocaml_format(context: &DiscoveryContext) -> Option<Vec<String>> {
    which_binary("ocamlformat").await?;
    let _config = find_up(".ocamlformat", &context.target_dir, &context.workspace_root)
        .await
        .into_iter()
        .next()?;
    Some(vec!["ocamlformat".into(), "-i".into(), "$FILE".into()])
}

async fn resolve_pint(context: &DiscoveryContext) -> Option<Vec<String>> {
    for composer_json in find_up(
        "composer.json",
        &context.target_dir,
        &context.workspace_root,
    )
    .await
    {
        let Some(composer): Option<ComposerJson> = read_json_file(&composer_json).await else {
            continue;
        };
        if has_dep(composer.require.as_ref(), "laravel/pint")
            || has_dep(composer.require_dev.as_ref(), "laravel/pint")
        {
            return Some(vec!["./vendor/bin/pint".into(), "$FILE".into()]);
        }
    }
    None
}

async fn resolve_air(_context: &DiscoveryContext) -> Option<Vec<String>> {
    let air = which_binary("air").await?;
    let check = run_command_check(air.to_string_lossy().to_string(), &["--help"]).await?;
    if check.exit_code != 0 {
        return None;
    }
    let first = first_line(&check.output);
    if first.contains("R language") && first.contains("formatter") {
        Some(vec![
            air.to_string_lossy().to_string(),
            "format".into(),
            "$FILE".into(),
        ])
    } else {
        None
    }
}

#[cfg(test)]
mod rule_tests;
#[cfg(test)]
mod tests;

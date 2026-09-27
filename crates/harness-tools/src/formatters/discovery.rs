use harness_core::{
    config::FormatterConfig,
    tool::{ToolContext, ToolError},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

const WEB: &str = "js jsx mjs cjs ts tsx mts cts html htm css scss sass less vue svelte json jsonc yaml yml toml xml md mdx graphql gql";
const BUILTINS: &[(&str, &str, &[&str])] = &[
    ("gofmt", "go", &["gofmt", "-w"]),
    ("mix", "ex exs eex heex leex neex sface", &["mix", "format"]),
    ("prettier", WEB, &["prettier", "--write"]),
    ("oxfmt", "js jsx mjs cjs ts tsx mts cts", &["oxfmt"]),
    ("biome", WEB, &["biome", "format", "--write"]),
    ("zig", "zig zon", &["zig", "fmt"]),
    (
        "clang-format",
        "c cc cpp cxx c++ h hh hpp hxx h++ ino",
        &["clang-format", "-i"],
    ),
    ("ktlint", "kt kts", &["ktlint", "-F"]),
    ("ruff", "py pyi", &["ruff", "format"]),
    ("air", "r", &["air", "format"]),
    ("uv", "py pyi", &["uv", "format", "--"]),
    (
        "rubocop",
        "rb rake gemspec ru",
        &["rubocop", "--autocorrect"],
    ),
    ("standardrb", "rb rake gemspec ru", &["standardrb", "--fix"]),
    ("htmlbeautifier", "erb", &["htmlbeautifier"]),
    ("dart", "dart", &["dart", "format"]),
    ("ocamlformat", "ml mli", &["ocamlformat", "-i"]),
    ("terraform", "tf tfvars", &["terraform", "fmt"]),
    ("latexindent", "tex", &["latexindent", "-w", "-s"]),
    ("gleam", "gleam", &["gleam", "format"]),
    ("shfmt", "sh bash", &["shfmt", "-w"]),
    ("nixfmt", "nix", &["nixfmt"]),
    (
        "rustfmt",
        "rs",
        &["rustfmt", "--config", "skip_children=true"],
    ),
    ("pint", "php", &["vendor/bin/pint"]),
    ("ormolu", "hs", &["ormolu", "-i"]),
    ("cljfmt", "clj cljs cljc edn", &["cljfmt", "fix", "--quiet"]),
    ("dfmt", "d", &["dfmt", "-i"]),
];

pub(super) struct Invocation {
    pub name: String,
    pub command: Vec<String>,
    pub environment: BTreeMap<String, String>,
}
pub(super) async fn resolve(
    context: &ToolContext,
    path: &Path,
) -> Result<Vec<Invocation>, ToolError> {
    let config = &context.formatter;
    let mut result = Vec::new();
    if !config.enabled {
        return Ok(result);
    }
    let Some(extension) = path.extension().and_then(|s| s.to_str()) else {
        return Ok(result);
    };
    let extension = extension.to_ascii_lowercase();
    let names = BUILTINS.iter().map(|(name, _, _)| *name).chain(
        config
            .overrides
            .keys()
            .map(String::as_str)
            .filter(|name| !BUILTINS.iter().any(|b| b.0 == *name)),
    );
    for name in names {
        if disabled(config, name) {
            continue;
        }
        let built = BUILTINS.iter().find(|b| b.0 == name);
        let custom = config.overrides.get(name);
        let matches = custom.and_then(|o| o.extensions.as_ref()).map_or_else(
            || built.is_some_and(|b| b.1.split_whitespace().any(|ext| ext == extension)),
            |extensions| {
                extensions
                    .iter()
                    .any(|ext| ext.trim_start_matches('.').eq_ignore_ascii_case(&extension))
            },
        );
        if !matches {
            continue;
        }
        let command = if let Some(command) = custom.and_then(|o| o.command.clone()) {
            command
        } else if let Some(built) = built {
            let Some(command) = discover(context, path, built, &result).await? else {
                continue;
            };
            command
        } else {
            continue;
        };
        let mut environment = custom
            .and_then(|o| o.environment.clone())
            .unwrap_or_default();
        if matches!(name, "prettier" | "biome" | "oxfmt") {
            environment.entry("BUN_BE_BUN".into()).or_insert("1".into());
        }
        result.push(Invocation {
            name: name.into(),
            command,
            environment,
        });
    }
    Ok(result)
}
fn disabled(config: &FormatterConfig, name: &str) -> bool {
    config.overrides.get(name).is_some_and(|o| o.disabled)
        || name == "oxfmt" && !config.experimental_oxfmt
        || matches!(name, "ruff" | "uv")
            && ["ruff", "uv"]
                .iter()
                .any(|name| config.overrides.get(*name).is_some_and(|o| o.disabled))
}
async fn discover(
    context: &ToolContext,
    path: &Path,
    &(name, _, template): &(&str, &str, &[&str]),
    resolved: &[Invocation],
) -> Result<Option<Vec<String>>, ToolError> {
    let root = &context.workspace_root;
    let folders: Vec<_> = path
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .take_while(|p| p.starts_with(root))
        .collect();
    let marker = |names: &[&str]| {
        folders
            .iter()
            .find(|dir| names.iter().any(|name| dir.join(name).is_file()))
            .copied()
    };
    let local = match name {
        "prettier" | "oxfmt" => folders
            .iter()
            .find(|dir| {
                dependency(
                    &dir.join("package.json"),
                    &["dependencies", "devDependencies"],
                    name,
                )
            })
            .copied(),
        "biome" => marker(&["biome.json", "biome.jsonc"]),
        "clang-format" => marker(&[".clang-format"]),
        "ocamlformat" => marker(&[".ocamlformat"]),
        "pint" => folders
            .iter()
            .find(|dir| {
                dependency(
                    &dir.join("composer.json"),
                    &["require", "require-dev"],
                    "laravel/pint",
                )
            })
            .copied(),
        "ruff" => folders
            .iter()
            .find(|dir| {
                ["ruff.toml", ".ruff.toml"]
                    .iter()
                    .any(|f| dir.join(f).is_file())
                    || ["pyproject.toml", "requirements.txt", "Pipfile"]
                        .iter()
                        .any(|f| read(&dir.join(f)).is_some_and(|s| s.contains("ruff")))
            })
            .copied(),
        _ => Some(root.as_path()),
    };
    let Some(local) = local else {
        return Ok(None);
    };
    if name == "uv" && resolved.iter().any(|f| f.name == "ruff") {
        return Ok(None);
    }
    let program = if name == "pint" {
        executable(&local.join("vendor/bin/pint"))
    } else if matches!(name, "prettier" | "biome" | "oxfmt") {
        local
            .ancestors()
            .take_while(|p| p.starts_with(root))
            .find_map(|dir| executable(&dir.join("node_modules/.bin").join(name)))
            .or_else(|| find(name, root))
    } else {
        find(name, root)
    };
    let Some(program) = program else {
        return Ok(None);
    };
    if matches!(name, "uv" | "air") {
        let mut command = tokio::process::Command::new(&program);
        if name == "uv" {
            command.arg("format");
        }
        command.arg("--help").current_dir(root);
        let output =
            match crate::process::run(command, Duration::from_secs(5), &context.cancellation).await
            {
                Ok(output) => output,
                Err(ToolError::Cancelled) => return Err(ToolError::Cancelled),
                Err(_) => return Ok(None),
            };
        let help = String::from_utf8_lossy(&output.stdout);
        if !output.status.success()
            || name == "air"
                && !help
                    .lines()
                    .next()
                    .is_some_and(|line| line.contains("R language") && line.contains("formatter"))
        {
            return Ok(None);
        }
    }
    let Some(program) = program.to_str() else {
        return Ok(None);
    };
    let mut command: Vec<_> = template.iter().map(|s| (*s).to_owned()).collect();
    command[0] = program.into();
    Ok(Some(command))
}
fn find(name: &str, root: &Path) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .find_map(|directory| executable(&root.join(directory).join(name)))
}
fn executable(path: &Path) -> Option<PathBuf> {
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(path.into())
}
fn read(path: &Path) -> Option<String> {
    let mut text = String::new();
    harness_core::store::open_private_file(path)
        .ok()?
        .take(128 * 1024 + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() <= 128 * 1024).then_some(text)
}
fn dependency(path: &Path, sections: &[&str], name: &str) -> bool {
    read(path)
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|json| {
            sections
                .iter()
                .any(|section| json[*section].get(name).is_some())
        })
}

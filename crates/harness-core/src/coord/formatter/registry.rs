// fragment the canonical registry order and command/extension mappings.
//! Built-in formatter registry for OpenCode-parity formatting.
//!
//! This module defines the canonical set of built-in formatters, their default
//! file extensions, optional environment variables, and command templates. Each
//! command must contain the `$FILE` placeholder, which the runner replaces with
//! the target path.

/// Discovery strategy for a built-in formatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryKind {
    /// Formatter is discovered by bare `which` (no special discovery logic).
    WhichOnly,
    /// Formatter uses Prettier-style discovery (package.json or installed globally).
    Prettier,
    /// Formatter uses Biome-style discovery (biome.json or installed globally).
    Biome,
    /// Formatter uses Oxfmt-style discovery (oxfmt binary on PATH).
    Oxfmt,
    /// Formatter uses ClangFormat-style discovery (.clang-format file or clang-format on PATH).
    ClangFormat,
    /// Formatter uses Ruff-style discovery (ruff.toml/pyproject.toml or ruff on PATH).
    Ruff,
    /// Formatter uses UvFormat-style discovery (uv on PATH).
    UvFormat,
    /// Formatter uses Ocamlformat-style discovery (.ocamlformat file or ocamlformat on PATH).
    Ocamlformat,
    /// Formatter uses Pint-style discovery (composer/pint on PATH).
    Pint,
    /// Formatter uses Air-style discovery (air on PATH).
    Air,
}

/// Metadata describing a single built-in formatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatterInfo {
    /// Human-readable formatter name, used as a registry key.
    pub name: &'static str,
    /// File extensions that this formatter handles, including the leading dot.
    pub extensions: &'static [&'static str],
    /// Optional environment variables to set when invoking the formatter.
    pub environment: Option<&'static [(&'static str, &'static str)]>,
    /// Command template. Must include exactly one `$FILE` argument.
    pub command: &'static [&'static str],
    /// How this formatter is discovered on the system.
    pub discovery_kind: DiscoveryKind,
}

// Shared extension sets for brevity.
const JS_TS_EXT: &[&str] = &[".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts"];
const PRETTIER_BIOME_EXT: &[&str] = &[
    ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts", ".html", ".htm", ".css", ".scss",
    ".sass", ".less", ".vue", ".svelte", ".json", ".jsonc", ".yaml", ".yml", ".toml", ".xml",
    ".md", ".mdx", ".graphql", ".gql",
];
const RUBY_EXT: &[&str] = &[".rb", ".rake", ".gemspec", ".ru"];
const BUN_ENV: Option<&[(&str, &str)]> = Some(&[("BUN_BE_BUN", "1")]);

/// Canonical built-in formatters matching OpenCode's default formatter set.
pub static BUILTIN_FORMATTERS: &[FormatterInfo] = &[
    FormatterInfo {
        name: "gofmt",
        extensions: &[".go"],
        environment: None,
        command: &["gofmt", "-w", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "mix",
        extensions: &[".ex", ".exs", ".eex", ".heex", ".leex", ".neex", ".sface"],
        environment: None,
        command: &["mix", "format", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "prettier",
        extensions: PRETTIER_BIOME_EXT,
        environment: BUN_ENV,
        command: &["prettier", "--write", "$FILE"],
        discovery_kind: DiscoveryKind::Prettier,
    },
    FormatterInfo {
        name: "oxfmt",
        extensions: JS_TS_EXT,
        environment: BUN_ENV,
        command: &["oxfmt", "$FILE"],
        discovery_kind: DiscoveryKind::Oxfmt,
    },
    FormatterInfo {
        name: "biome",
        extensions: PRETTIER_BIOME_EXT,
        environment: BUN_ENV,
        command: &["biome", "format", "--write", "$FILE"],
        discovery_kind: DiscoveryKind::Biome,
    },
    FormatterInfo {
        name: "zig",
        extensions: &[".zig", ".zon"],
        environment: None,
        command: &["zig", "fmt", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "clang-format",
        extensions: &[
            ".c", ".cc", ".cpp", ".cxx", ".c++", ".h", ".hh", ".hpp", ".hxx", ".h++", ".ino", ".C",
            ".H",
        ],
        environment: None,
        command: &["clang-format", "-i", "$FILE"],
        discovery_kind: DiscoveryKind::ClangFormat,
    },
    FormatterInfo {
        name: "ktlint",
        extensions: &[".kt", ".kts"],
        environment: None,
        command: &["ktlint", "-F", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "ruff",
        extensions: &[".py", ".pyi"],
        environment: None,
        command: &["ruff", "format", "$FILE"],
        discovery_kind: DiscoveryKind::Ruff,
    },
    FormatterInfo {
        name: "air",
        extensions: &[".R"],
        environment: None,
        command: &["air", "format", "$FILE"],
        discovery_kind: DiscoveryKind::Air,
    },
    FormatterInfo {
        name: "uv",
        extensions: &[".py", ".pyi"],
        environment: None,
        command: &["uv", "format", "--", "$FILE"],
        discovery_kind: DiscoveryKind::UvFormat,
    },
    FormatterInfo {
        name: "rubocop",
        extensions: RUBY_EXT,
        environment: None,
        command: &["rubocop", "--autocorrect", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "standardrb",
        extensions: RUBY_EXT,
        environment: None,
        command: &["standardrb", "--fix", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "htmlbeautifier",
        extensions: &[".erb", ".html.erb"],
        environment: None,
        command: &["htmlbeautifier", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "dart",
        extensions: &[".dart"],
        environment: None,
        command: &["dart", "format", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "ocamlformat",
        extensions: &[".ml", ".mli"],
        environment: None,
        command: &["ocamlformat", "-i", "$FILE"],
        discovery_kind: DiscoveryKind::Ocamlformat,
    },
    FormatterInfo {
        name: "terraform",
        extensions: &[".tf", ".tfvars"],
        environment: None,
        command: &["terraform", "fmt", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "latexindent",
        extensions: &[".tex"],
        environment: None,
        command: &["latexindent", "-w", "-s", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "gleam",
        extensions: &[".gleam"],
        environment: None,
        command: &["gleam", "format", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "shfmt",
        extensions: &[".sh", ".bash"],
        environment: None,
        command: &["shfmt", "-w", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "nixfmt",
        extensions: &[".nix"],
        environment: None,
        command: &["nixfmt", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "rustfmt",
        extensions: &[".rs"],
        environment: None,
        command: &["rustfmt", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "pint",
        extensions: &[".php"],
        environment: None,
        command: &["./vendor/bin/pint", "$FILE"],
        discovery_kind: DiscoveryKind::Pint,
    },
    FormatterInfo {
        name: "ormolu",
        extensions: &[".hs"],
        environment: None,
        command: &["ormolu", "-i", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "cljfmt",
        extensions: &[".clj", ".cljs", ".cljc", ".edn"],
        environment: None,
        command: &["cljfmt", "fix", "--quiet", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
    FormatterInfo {
        name: "dfmt",
        extensions: &[".d"],
        environment: None,
        command: &["dfmt", "-i", "$FILE"],
        discovery_kind: DiscoveryKind::WhichOnly,
    },
];

/// Return the first built-in formatter that handles `extension`.
///
/// `extension` may be passed with or without a leading dot; the registry
/// extensions always include the dot.
pub fn formatter_info_by_extension(extension: &str) -> Option<&'static FormatterInfo> {
    let needle = extension.strip_prefix('.').unwrap_or(extension);
    BUILTIN_FORMATTERS.iter().find(|info| {
        info.extensions
            .iter()
            .any(|ext| ext.strip_prefix('.').is_some_and(|e| e == needle))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    #[test]
    fn formatter_registry_contains_all_expected_names() {
        // arrange
        let names: Vec<_> = BUILTIN_FORMATTERS.iter().map(|info| info.name).collect();
        // act
        for expected in [
            "gofmt",
            "mix",
            "prettier",
            "oxfmt",
            "biome",
            "zig",
            "clang-format",
            "ktlint",
            "ruff",
            "air",
            "uv",
            "rubocop",
            "standardrb",
            "htmlbeautifier",
            "dart",
            "ocamlformat",
            "terraform",
            "latexindent",
            "gleam",
            "shfmt",
            "nixfmt",
            "rustfmt",
            "pint",
            "ormolu",
            "cljfmt",
            "dfmt",
        ] {
            assert!(names.contains(&expected), "missing formatter: {expected}");
        }
        // assert
        assert_eq!(
            names.len(),
            26,
            "registry should contain exactly 26 formatters"
        );
    }

    #[test]
    fn formatter_registry_entries_have_extensions_and_file_placeholder() {
        // arrange
        // act
        for info in BUILTIN_FORMATTERS {
            // assert
            assert!(
                !info.extensions.is_empty(),
                "{} has no extensions",
                info.name
            );
            assert!(
                info.command.contains(&"$FILE"),
                "{} command missing $FILE",
                info.name
            );
        }
    }

    #[test]
    fn formatter_registry_extensions_include_leading_dot() {
        // arrange
        // act
        for info in BUILTIN_FORMATTERS {
            // assert
            for ext in info.extensions {
                assert!(
                    ext.starts_with('.'),
                    "{} extension {ext} missing leading dot",
                    info.name
                );
            }
        }
    }

    #[test]
    fn formatter_registry_prettier_biome_oxfmt_set_bun_be_bun() {
        // arrange
        // act
        for name in ["prettier", "biome", "oxfmt"] {
            let info = BUILTIN_FORMATTERS
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{name} not found"));
            let env = info
                .environment
                .unwrap_or_else(|| panic!("{name} missing environment"));
            // assert
            assert!(
                env.contains(&("BUN_BE_BUN", "1")),
                "{name} missing BUN_BE_BUN=1"
            );
        }
    }

    #[test]
    fn formatter_registry_lookup_by_extension_finds_match() {
        // arrange
        // act
        let rust = formatter_info_by_extension("rs").unwrap_or_abort();
        // assert
        assert_eq!(rust.name, "rustfmt");
        let python = formatter_info_by_extension(".py").unwrap_or_abort();
        assert_eq!(python.name, "ruff");
        assert!(formatter_info_by_extension("not-an-ext").is_none());
    }
}

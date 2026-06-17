//! Built-in formatter registry for OpenCode-parity formatting.
//!
//! This module defines the canonical set of built-in formatters, their default
//! file extensions, optional environment variables, and command templates. Each
//! command must contain the `$FILE` placeholder, which the runner replaces with
//! the target path.

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
}

/// Canonical built-in formatters matching OpenCode's default formatter set.
pub static BUILTIN_FORMATTERS: &[FormatterInfo] = &[
    FormatterInfo {
        name: "rustfmt",
        extensions: &[".rs"],
        environment: None,
        command: &["rustfmt", "$FILE"],
    },
    FormatterInfo {
        name: "ruff",
        extensions: &[".py", ".pyi"],
        environment: None,
        command: &["ruff", "format", "$FILE"],
    },
    FormatterInfo {
        name: "uvformat",
        extensions: &[".py", ".pyi"],
        environment: None,
        command: &["uv", "format", "$FILE"],
    },
    FormatterInfo {
        name: "prettier",
        extensions: &[
            ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts", ".html", ".htm", ".css",
            ".scss", ".sass", ".less", ".vue", ".svelte", ".json", ".jsonc", ".yaml", ".yml",
            ".toml", ".xml", ".md", ".mdx", ".graphql", ".gql",
        ],
        environment: Some(&[("BUN_BE_BUN", "1")]),
        command: &["prettier", "--write", "$FILE"],
    },
    FormatterInfo {
        name: "biome",
        extensions: &[
            ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts", ".html", ".htm", ".css",
            ".scss", ".sass", ".less", ".vue", ".svelte", ".json", ".jsonc", ".yaml", ".yml",
            ".toml", ".xml", ".md", ".mdx", ".graphql", ".gql",
        ],
        environment: Some(&[("BUN_BE_BUN", "1")]),
        command: &["biome", "format", "--write", "$FILE"],
    },
    FormatterInfo {
        name: "gofmt",
        extensions: &[".go"],
        environment: None,
        command: &["gofmt", "-w", "$FILE"],
    },
    FormatterInfo {
        name: "zig",
        extensions: &[".zig", ".zon"],
        environment: None,
        command: &["zig", "fmt", "$FILE"],
    },
    FormatterInfo {
        name: "dart",
        extensions: &[".dart"],
        environment: None,
        command: &["dart", "format", "$FILE"],
    },
    FormatterInfo {
        name: "shfmt",
        extensions: &[".sh", ".bash"],
        environment: None,
        command: &["shfmt", "-w", "$FILE"],
    },
    FormatterInfo {
        name: "nixfmt",
        extensions: &[".nix"],
        environment: None,
        command: &["nixfmt", "$FILE"],
    },
];

/// Return the first built-in formatter that handles `extension`.
///
/// `extension` may be passed with or without a leading dot; the registry
/// extensions always include the dot.
#[allow(dead_code)]
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

    #[test]
    fn formatter_registry_contains_all_expected_names() {
        let names: Vec<_> = BUILTIN_FORMATTERS.iter().map(|info| info.name).collect();
        for expected in [
            "rustfmt", "ruff", "uvformat", "prettier", "biome", "gofmt", "zig", "dart", "shfmt",
            "nixfmt",
        ] {
            assert!(names.contains(&expected), "missing formatter: {expected}");
        }
    }

    #[test]
    fn formatter_registry_entries_have_extensions_and_file_placeholder() {
        for info in BUILTIN_FORMATTERS {
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
        for info in BUILTIN_FORMATTERS {
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
    fn formatter_registry_prettier_and_biome_set_bun_be_bun() {
        for name in ["prettier", "biome"] {
            let info = BUILTIN_FORMATTERS
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{name} not found"));
            let env = info
                .environment
                .unwrap_or_else(|| panic!("{name} missing environment"));
            assert!(
                env.contains(&("BUN_BE_BUN", "1")),
                "{name} missing BUN_BE_BUN=1"
            );
        }
    }

    #[test]
    fn formatter_registry_lookup_by_extension_finds_match() {
        let rust = formatter_info_by_extension("rs").expect("lookup rs");
        assert_eq!(rust.name, "rustfmt");

        let python = formatter_info_by_extension(".py").expect("lookup .py");
        assert_eq!(python.name, "ruff");

        assert!(formatter_info_by_extension("not-an-ext").is_none());
    }
}

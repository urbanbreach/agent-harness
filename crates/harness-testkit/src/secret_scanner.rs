use crate::UnwrapOrAbort;
use regex::Regex;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ForbiddenPattern {
    Regex { name: String, expression: Regex },
    Substring { name: String, needle: String },
}

impl ForbiddenPattern {
    pub fn regex(name: impl Into<String>, expression: &str) -> Result<Self, regex::Error> {
        Ok(Self::Regex {
            name: name.into(),
            expression: Regex::new(expression)?,
        })
    }

    pub fn substring(name: impl Into<String>, needle: impl Into<String>) -> Self {
        Self::Substring {
            name: name.into(),
            needle: needle.into(),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Regex { name, .. } | Self::Substring { name, .. } => name,
        }
    }

    fn matches(&self, line: &str) -> bool {
        match self {
            Self::Regex { expression, .. } => expression.is_match(line),
            Self::Substring { needle, .. } => line.contains(needle),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretFinding {
    pub pattern: String,
    pub file: PathBuf,
    pub line_number: usize,
}

pub fn default_forbidden_patterns() -> Result<Vec<ForbiddenPattern>, regex::Error> {
    Ok(vec![
        ForbiddenPattern::regex("openai_api_key", r"\bsk-[A-Za-z0-9_-]{10,}\b")?,
        ForbiddenPattern::regex("anthropic_api_key", r"\bsk-ant-[A-Za-z0-9_-]{10,}\b")?,
        ForbiddenPattern::regex("google_api_key", r"\bAIza[0-9A-Za-z_-]{20,}\b")?,
        ForbiddenPattern::regex("aws_access_key_id", r"\bAKIA[0-9A-Z]{16}\b")?,
        ForbiddenPattern::regex("github_pat", r"\bgithub_pat_[A-Za-z0-9_]{20,}\b")?,
        ForbiddenPattern::regex("github_token", r"\bghp_[A-Za-z0-9]{20,}\b")?,
        ForbiddenPattern::regex(
            "authorization_bearer",
            r#"(?i)\bauthorization"?\s*:\s*"?bearer\s+[A-Za-z0-9._~+/=-]{8,}"#,
        )?,
        ForbiddenPattern::regex("pem_private_key", r"-----BEGIN [A-Z ]*PRIVATE KEY-----")?,
        ForbiddenPattern::substring("Bearer sk-", "Bearer sk-"),
    ])
}

pub fn forbidden_patterns_with_env_values<I, K, V>(
    vars: I,
) -> Result<Vec<ForbiddenPattern>, regex::Error>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut patterns = default_forbidden_patterns()?;
    patterns.extend(env_credential_patterns(vars));
    Ok(patterns)
}

pub fn env_credential_patterns<I, K, V>(vars: I) -> Vec<ForbiddenPattern>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    vars.into_iter()
        .filter_map(|(key, value)| {
            let key = key.as_ref();
            let value = value.as_ref();
            if !looks_like_credential_name(key) || value.len() < 8 {
                return None;
            }
            Some(ForbiddenPattern::substring(
                format!("env:{key}"),
                value.to_owned(),
            ))
        })
        .collect()
}

pub fn scan_directory_tree_for_secrets(
    root: &Path,
    patterns: &[ForbiddenPattern],
) -> io::Result<Vec<SecretFinding>> {
    let mut findings = Vec::new();
    scan_path(root, patterns, &mut findings)?;
    Ok(findings)
}

pub fn scan_directories_for_secrets<I, P>(
    roots: I,
    patterns: &[ForbiddenPattern],
) -> io::Result<Vec<SecretFinding>>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut findings = Vec::new();
    for root in roots {
        scan_path(root.as_ref(), patterns, &mut findings)?;
    }
    Ok(findings)
}

fn scan_path(
    path: &Path,
    patterns: &[ForbiddenPattern],
    findings: &mut Vec<SecretFinding>,
) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        return Ok(());
    }

    if metadata.is_file() {
        scan_file(path, patterns, findings)?;
        return Ok(());
    }

    if metadata.is_dir() {
        for entry in read_dir_sorted(path)? {
            scan_path(&entry, patterns, findings)?;
        }
    }

    Ok(())
}

fn scan_file(
    path: &Path,
    patterns: &[ForbiddenPattern],
    findings: &mut Vec<SecretFinding>,
) -> io::Result<()> {
    let bytes = fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);

    for (line_idx, line) in text.lines().enumerate() {
        let line_number = line_idx + 1;
        for pattern in patterns {
            if pattern.matches(line) {
                findings.push(SecretFinding {
                    pattern: pattern.name().to_owned(),
                    file: path.to_path_buf(),
                    line_number,
                });
            }
        }
    }

    Ok(())
}

fn read_dir_sorted(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
        .map(|entry| entry.map(|item| item.path()))
        .collect::<Result<_, _>>()?;
    paths.sort();
    Ok(paths)
}

fn looks_like_credential_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "CREDENTIAL",
        "API_KEY",
        "ACCESS_KEY",
    ]
    .iter()
    .any(|needle| upper.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::{
        default_forbidden_patterns, env_credential_patterns, scan_directory_tree_for_secrets,
    };
    use crate::UnwrapOrAbort;

    #[test]
    fn default_patterns_detect_common_cassette_secret_shapes() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let cassette = temp.path().join("cassette.json");
        std::fs::write(
            &cassette,
            r#"{"Authorization":"Bearer sk-ant-secret00000000000000000000"}"#,
        )
        .unwrap_or_abort();

        let findings = scan_directory_tree_for_secrets(
            temp.path(),
            &default_forbidden_patterns().unwrap_or_abort(),
        )
        .unwrap_or_abort();

        assert!(findings
            .iter()
            .any(|finding| finding.pattern == "anthropic_api_key"));
        assert!(findings
            .iter()
            .any(|finding| finding.pattern == "authorization_bearer"));
    }

    #[test]
    fn env_credential_patterns_only_use_credential_named_values() {
        let patterns = env_credential_patterns([
            ("OPENAI_API_KEY", "sk-live-secret"),
            ("OPENAI_KEY", "plain-env-secret-value"),
            ("ORDINARY_VALUE", "sk-live-secret"),
            ("SHORT_TOKEN", "short"),
        ]);

        assert_eq!(patterns.len(), 2);
        let temp = tempfile::tempdir().unwrap_or_abort();
        let cassette = temp.path().join("cassette.json");
        std::fs::write(&cassette, "sk-live-secret").unwrap_or_abort();

        let findings = scan_directory_tree_for_secrets(temp.path(), &patterns).unwrap_or_abort();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].pattern, "env:OPENAI_API_KEY");

        std::fs::write(&cassette, "plain-env-secret-value").unwrap_or_abort();
        let findings = scan_directory_tree_for_secrets(temp.path(), &patterns).unwrap_or_abort();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].pattern, "env:OPENAI_KEY");
    }
}

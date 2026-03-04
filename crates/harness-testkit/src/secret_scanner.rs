use regex::Regex;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ForbiddenPattern {
    Regex {
        name: &'static str,
        expression: Regex,
    },
    Substring {
        name: &'static str,
        needle: &'static str,
    },
}

impl ForbiddenPattern {
    fn name(&self) -> &'static str {
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
    pub pattern: &'static str,
    pub file: PathBuf,
    pub line_number: usize,
}

pub fn default_forbidden_patterns() -> Vec<ForbiddenPattern> {
    vec![
        ForbiddenPattern::Regex {
            name: "sk-[A-Za-z0-9]{10,}",
            expression: Regex::new(r"sk-[A-Za-z0-9]{10,}").expect("valid sk token regex"),
        },
        ForbiddenPattern::Substring {
            name: "Authorization: Bearer",
            needle: "Authorization: Bearer",
        },
        ForbiddenPattern::Substring {
            name: "Bearer sk-",
            needle: "Bearer sk-",
        },
    ]
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
                    pattern: pattern.name(),
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

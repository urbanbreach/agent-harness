use std::path::PathBuf;

use async_trait::async_trait;

#[cfg(test)]
use super::BUILTIN_FORMATTERS;

#[cfg(test)]
use std::collections::HashSet;

/// Context passed to formatter discovery.
#[derive(Debug, Clone)]
pub struct DiscoveryContext {
    pub workspace_root: PathBuf,
    pub target_dir: PathBuf,
    pub experimental_oxfmt: bool,
}

/// Discovers and resolves a command vector for a named formatter.
#[async_trait]
pub trait FormatterDiscovery: Send + Sync {
    /// Resolve the command vector for a named formatter, or `None` if unavailable.
    /// When `Some(command)` is returned, every command must contain `"$FILE"`.
    async fn resolve(&self, name: &str, context: &DiscoveryContext) -> Option<Vec<String>>;
}

/// Deterministic discovery for tests.
#[derive(Debug, Clone, Default)]
#[cfg(test)]
pub struct FakeFormatterDiscovery {
    names: HashSet<String>,
}

#[cfg(test)]
impl FakeFormatterDiscovery {
    pub fn new(names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            names: names.into_iter().map(Into::into).collect(),
        }
    }

    pub fn insert(&mut self, name: impl Into<String>) {
        self.names.insert(name.into());
    }

    pub fn remove(&mut self, name: &str) {
        self.names.remove(name);
    }
}

#[async_trait]
#[cfg(test)]
impl FormatterDiscovery for FakeFormatterDiscovery {
    async fn resolve(&self, name: &str, _context: &DiscoveryContext) -> Option<Vec<String>> {
        if !self.names.contains(name) {
            return None;
        }
        let info = BUILTIN_FORMATTERS.iter().find(|info| info.name == name)?;
        let mut command: Vec<String> = info.command.iter().map(|arg| (*arg).to_string()).collect();
        if command.is_empty() {
            return None;
        }
        command[0] = name.to_string();
        Some(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_discovery_reports_configured_names() {
        // arrange
        let context = DiscoveryContext {
            workspace_root: PathBuf::from("."),
            target_dir: PathBuf::from("."),
            experimental_oxfmt: false,
        };
        let discovery = FakeFormatterDiscovery::new(["rustfmt", "prettier"]);
        // act
        // assert
        assert!(discovery.resolve("rustfmt", &context).await.is_some());
        assert!(discovery.resolve("prettier", &context).await.is_some());
        assert!(discovery.resolve("ruff", &context).await.is_none());
    }

    #[tokio::test]
    async fn fake_discovery_supports_runtime_mutation() {
        // arrange
        let context = DiscoveryContext {
            workspace_root: PathBuf::from("."),
            target_dir: PathBuf::from("."),
            experimental_oxfmt: false,
        };
        let mut discovery = FakeFormatterDiscovery::new(["rustfmt"]);
        // act
        assert!(discovery.resolve("rustfmt", &context).await.is_some());
        discovery.remove("rustfmt");
        discovery.insert("ruff");
        // assert
        assert!(discovery.resolve("rustfmt", &context).await.is_none());
        assert!(discovery.resolve("ruff", &context).await.is_some());
    }
}

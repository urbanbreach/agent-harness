use async_trait::async_trait;

#[cfg(test)]
use std::collections::HashSet;

/// Discovers whether a named formatter binary is available on the current PATH.
#[async_trait]
pub trait FormatterDiscovery: Send + Sync {
    /// Return `true` if `name` is available as an executable on PATH.
    async fn is_on_path(&self, name: &str) -> bool;
}

/// PATH-based discovery using the `which` crate.
#[derive(Debug, Clone, Copy, Default)]
pub struct WhichFormatterDiscovery;

#[async_trait]
impl FormatterDiscovery for WhichFormatterDiscovery {
    async fn is_on_path(&self, name: &str) -> bool {
        let name = name.to_string();
        matches!(
            tokio::task::spawn_blocking(move || which::which(&name)).await,
            Ok(Ok(_))
        )
    }
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
    async fn is_on_path(&self, name: &str) -> bool {
        self.names.contains(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_discovery_reports_configured_names() {
        let discovery = FakeFormatterDiscovery::new(["rustfmt", "prettier"]);
        assert!(discovery.is_on_path("rustfmt").await);
        assert!(discovery.is_on_path("prettier").await);
        assert!(!discovery.is_on_path("ruff").await);
    }

    #[tokio::test]
    async fn fake_discovery_supports_runtime_mutation() {
        let mut discovery = FakeFormatterDiscovery::new(["rustfmt"]);
        assert!(discovery.is_on_path("rustfmt").await);
        discovery.remove("rustfmt");
        assert!(!discovery.is_on_path("rustfmt").await);
        discovery.insert("ruff");
        assert!(discovery.is_on_path("ruff").await);
    }
}

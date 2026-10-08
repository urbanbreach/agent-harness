use harness_core::{
    clock::{Clock, FakeClock, RealClock},
    config::ConfigLoadContext,
};
use harness_providers::Provider;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

#[derive(Clone, Default)]
pub struct CliDeps {
    directory: Option<PathBuf>,
    environment: BTreeMap<String, Option<String>>,
    provider: Option<Arc<dyn Provider>>,
    cancellation: Option<tokio_util::sync::CancellationToken>,
    clock_factory: Option<Arc<dyn Fn(bool) -> Arc<dyn Clock + Send + Sync> + Send + Sync>>,
    /// Lines an interactive surface (the TUI) feeds a running login, such as a pasted code.
    interactive_input: Option<Arc<std::sync::Mutex<std::sync::mpsc::Receiver<String>>>>,
}
impl CliDeps {
    pub fn real() -> Self {
        Self::default()
    }
    pub fn with_current_dir(mut self, directory: PathBuf) -> Self {
        self.directory = Some(directory);
        self
    }
    pub fn with_filesystem_root(self, root: PathBuf) -> Self {
        self.with_current_dir(root)
    }
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(name.into(), Some(value.into()));
        self
    }
    pub fn without_env(mut self, name: impl Into<String>) -> Self {
        self.environment.insert(name.into(), None);
        self
    }
    pub fn with_provider_override(mut self, provider: Arc<dyn Provider>) -> Self {
        self.provider = Some(provider);
        self
    }
    pub fn with_cancellation(mut self, cancellation: tokio_util::sync::CancellationToken) -> Self {
        self.cancellation = Some(cancellation);
        self
    }
    pub fn with_interactive_input(mut self, input: std::sync::mpsc::Receiver<String>) -> Self {
        self.interactive_input = Some(Arc::new(std::sync::Mutex::new(input)));
        self
    }
    pub(crate) fn interactive_input(
        &self,
    ) -> Option<Arc<std::sync::Mutex<std::sync::mpsc::Receiver<String>>>> {
        self.interactive_input.clone()
    }
    pub(crate) fn cancellation(&self) -> Option<tokio_util::sync::CancellationToken> {
        self.cancellation.clone()
    }
    pub fn with_clock_factory(
        mut self,
        factory: impl Fn(bool) -> Arc<dyn Clock + Send + Sync> + Send + Sync + 'static,
    ) -> Self {
        self.clock_factory = Some(Arc::new(factory));
        self
    }
    pub fn current_dir(&self) -> std::io::Result<PathBuf> {
        match &self.directory {
            Some(path) if path.is_absolute() => Ok(path.clone()),
            Some(path) => std::env::current_dir().map(|base| base.join(path)),
            None => std::env::current_dir(),
        }
    }
    pub fn env_var_value(&self, name: &str) -> Option<String> {
        self.environment
            .get(name)
            .cloned()
            .unwrap_or_else(|| std::env::var(name).ok())
    }
    pub fn credential_env_values(&self) -> Vec<String> {
        std::env::vars_os()
            .filter_map(|(name, _)| name.into_string().ok())
            .chain(self.environment.keys().cloned())
            .filter(|name| {
                let name = name.to_ascii_uppercase();
                [
                    "API_KEY",
                    "APIKEY",
                    "TOKEN",
                    "SECRET",
                    "PASSWORD",
                    "CREDENTIAL",
                ]
                .iter()
                .any(|part| name.contains(part))
            })
            .filter_map(|name| self.env_var_value(&name))
            .filter(|value| !value.is_empty())
            .collect()
    }
    /// The process environment with this context's overrides applied.
    pub fn environment_snapshot(&self) -> std::collections::BTreeMap<String, String> {
        let mut environment: std::collections::BTreeMap<String, String> =
            std::env::vars().collect();
        for (name, value) in &self.environment {
            match value {
                Some(value) => environment.insert(name.clone(), value.clone()),
                None => environment.remove(name),
            };
        }
        environment
    }
    pub(crate) fn env_var_is_set(&self, name: &str) -> bool {
        self.env_var_value(name).is_some()
    }
    pub(crate) fn session_directory(
        &self,
        configured: &std::path::Path,
    ) -> Result<PathBuf, String> {
        let root = self.current_dir().map_err(|error| error.to_string())?;
        let data_dir =
            harness_core::storage_paths::data_dir_from_lookup(&|name| self.env_var_value(name));
        harness_core::storage_paths::resolve_session_dir(&root, configured, data_dir.as_deref())
            .map_err(|error| error.to_string())
    }
    pub(crate) fn data_directory(&self) -> Result<PathBuf, String> {
        harness_core::storage_paths::data_dir_from_lookup(&|name| self.env_var_value(name))
            .ok_or_else(|| "cannot resolve harness data directory; set HARNESS_DATA_HOME or provide --session-dir".to_string())
    }
    pub(crate) fn project_paths(
        &self,
        project: &std::path::Path,
    ) -> Result<harness_core::storage_paths::ProjectPaths, String> {
        harness_core::storage_paths::ProjectPaths::new(&self.data_directory()?, project)
            .map_err(|error| error.to_string())
    }
    pub(crate) fn config_load_context(&self) -> ConfigLoadContext {
        let mut context = ConfigLoadContext::from_env();
        if let Some(directory) = &self.directory {
            context.discovery.current_dir = context.discovery.current_dir.join(directory);
        }
        for (name, value) in &self.environment {
            context = context.apply_env_var(name, value.clone());
        }
        context.discovery.data_dir =
            harness_core::storage_paths::data_dir_from_lookup(&|name| self.env_var_value(name));
        context
    }
    pub fn provider_override(&self) -> Option<Arc<dyn Provider>> {
        self.provider.clone()
    }
    pub(crate) fn clock(&self, deterministic: bool) -> Arc<dyn Clock + Send + Sync> {
        if let Some(factory) = &self.clock_factory {
            factory(deterministic)
        } else if deterministic {
            Arc::new(FakeClock::new())
        } else {
            Arc::new(RealClock::new())
        }
    }
}

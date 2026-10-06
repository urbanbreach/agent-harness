use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EvalConfig {
    pub languages: Vec<String>,
    /// Tool IDs or discovery catalog IDs advertised through eval when it is allowed.
    pub route_tools: Vec<String>,
    pub cell_timeout_seconds: u64,
    pub foreground_window_seconds: u64,
    pub run_budget_seconds: u64,
    pub hard_limit_seconds: u64,
    pub max_detached_cells: usize,
    pub parallel_pool_width: usize,
    pub output_head_bytes: usize,
    pub output_max_columns: usize,
    pub status_events: bool,
    pub memory: EvalMemoryConfig,
    pub sandbox: EvalSandboxConfig,
}

impl Default for EvalConfig {
    fn default() -> Self {
        Self {
            languages: vec!["js".into(), "py".into()],
            route_tools: Vec::new(),
            cell_timeout_seconds: 30,
            foreground_window_seconds: 60,
            run_budget_seconds: 300,
            hard_limit_seconds: 1800,
            max_detached_cells: 15,
            parallel_pool_width: 4,
            output_head_bytes: 20480,
            output_max_columns: 768,
            status_events: true,
            memory: EvalMemoryConfig::default(),
            sandbox: EvalSandboxConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EvalSandboxConfig {
    pub enabled: bool,
    pub memory_limit_mb: usize,
    pub timeout_seconds: u64,
}
impl Default for EvalSandboxConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            memory_limit_mb: 64,
            timeout_seconds: 300,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EvalMemoryConfig {
    pub gc_watermark_mb: usize,
    pub notice_mb: usize,
    pub ceiling_mb: usize,
    pub retained_results_mb: usize,
    pub retained_images_mb: usize,
}
impl Default for EvalMemoryConfig {
    fn default() -> Self {
        Self {
            gc_watermark_mb: 256,
            notice_mb: 1024,
            ceiling_mb: 2048,
            retained_results_mb: 32,
            retained_images_mb: 256,
        }
    }
}

impl EvalConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(1..=4096).contains(&self.sandbox.memory_limit_mb)
            || !(1..=86400).contains(&self.sandbox.timeout_seconds)
        {
            return Err(ConfigError(
                "eval.sandbox requires memory_limit_mb 1-4096 and timeout_seconds 1-86400".into(),
            ));
        }
        if self.route_tools.len() > 256
            || self
                .route_tools
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.route_tools.len()
            || self.route_tools.iter().any(|name| {
                name.is_empty()
                    || name.len() > 256
                    || name.chars().any(char::is_whitespace)
                    || matches!(name.as_str(), "eval" | "question")
            })
        {
            return Err(ConfigError(
                "eval.route_tools must contain at most 256 unique tool or catalog IDs; eval and question must stay direct".into(),
            ));
        }
        if self.languages.is_empty()
            || self
                .languages
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.languages.len()
            || self
                .languages
                .iter()
                .any(|l| !matches!(l.as_str(), "js" | "py" | "rb" | "jl"))
        {
            return Err(ConfigError(
                "eval.languages must contain unique entries from js, py, rb or jl".into(),
            ));
        }
        if [
            self.cell_timeout_seconds,
            self.foreground_window_seconds,
            self.run_budget_seconds,
            self.hard_limit_seconds,
        ]
        .into_iter()
        .any(|n| n == 0 || n > 86400)
            || !(1..=256).contains(&self.max_detached_cells)
            || !(1..=256).contains(&self.parallel_pool_width)
            || self.output_head_bytes > 51200
            || self.output_max_columns > 16384
        {
            return Err(ConfigError("eval deadlines must be 1–86400 seconds, capacities 1–256, head output at most 51200 bytes and columns at most 16384".into()));
        }
        let thresholds = [
            self.memory.gc_watermark_mb,
            self.memory.notice_mb,
            self.memory.ceiling_mb,
        ];
        let enabled: Vec<_> = thresholds.into_iter().filter(|n| *n != 0).collect();
        if enabled.windows(2).any(|pair| pair[0] > pair[1])
            || thresholds
                .into_iter()
                .chain([
                    self.memory.retained_results_mb,
                    self.memory.retained_images_mb,
                ])
                .any(|n| n > 1_048_576)
        {
            return Err(ConfigError("eval memory limits must be at most 1048576 MiB; nonzero thresholds must satisfy gc_watermark_mb <= notice_mb <= ceiling_mb".into()));
        }
        Ok(())
    }
}

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

#[derive(Clone, Debug)]
pub struct SessionOptions {
    pub environment: BTreeMap<OsString, OsString>,
    pub cwd: PathBuf,
    pub artifacts: PathBuf,
    pub local_dir: PathBuf,
    pub languages: Vec<String>,
    pub session_env: BTreeMap<String, String>,
    pub settings: Settings,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub cell_timeout_seconds: u64,
    pub foreground_window_seconds: u64,
    pub run_budget_seconds: u64,
    pub hard_limit_seconds: u64,
    pub max_detached_cells: usize,
    pub parallel_pool_width: usize,
    pub output_head_bytes: usize,
    pub output_max_columns: usize,
    pub status_events: bool,
    pub memory: MemorySettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            cell_timeout_seconds: 30,
            foreground_window_seconds: 60,
            run_budget_seconds: 300,
            hard_limit_seconds: 1800,
            max_detached_cells: 15,
            parallel_pool_width: 4,
            output_head_bytes: 20 * 1024,
            output_max_columns: 768,
            status_events: true,
            memory: MemorySettings::default(),
        }
    }
}

impl Settings {
    pub(crate) fn validate(&self) -> crate::Result<()> {
        if [
            self.cell_timeout_seconds,
            self.foreground_window_seconds,
            self.run_budget_seconds,
            self.hard_limit_seconds,
        ]
        .iter()
        .any(|value| !(1..=86400).contains(value))
            || !(1..=256).contains(&self.max_detached_cells)
            || !(1..=256).contains(&self.parallel_pool_width)
            || self.output_head_bytes > 51200
            || self.output_max_columns > 16384
        {
            return Err("invalid eval deadlines, capacities or output limits".into());
        }
        let limits = [
            self.memory.gc_watermark_mb,
            self.memory.notice_mb,
            self.memory.ceiling_mb,
            self.memory.retained_results_mb,
            self.memory.retained_images_mb,
        ];
        let thresholds: Vec<_> = limits[..3].iter().filter(|value| **value != 0).collect();
        if limits.iter().any(|value| *value > 1_048_576)
            || thresholds.windows(2).any(|pair| pair[0] > pair[1])
        {
            return Err("invalid eval memory thresholds".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MemorySettings {
    pub gc_watermark_mb: usize,
    pub notice_mb: usize,
    pub ceiling_mb: usize,
    pub retained_results_mb: usize,
    pub retained_images_mb: usize,
}
impl Default for MemorySettings {
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

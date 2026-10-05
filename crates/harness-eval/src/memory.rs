use crate::MemorySettings;
use serde_json::{json, Value};

#[derive(Default)]
pub(crate) struct Policy {
    observed: u64,
    notified: u64,
    pub pending: Option<Value>,
    pub restarted: Option<Value>,
}

impl Policy {
    pub fn annotate(&mut self, language: &str, settings: &MemorySettings, report: &mut Value) {
        let live = report["liveBytes"].as_u64().unwrap_or(0);
        let notice = (settings.notice_mb as u64).saturating_mul(1024 * 1024);
        let ceiling = (settings.ceiling_mb as u64).saturating_mul(1024 * 1024);
        let crossed = self.observed < notice;
        self.observed = live;
        if live < notice / 2 {
            self.notified = 0;
        }
        let measure = if matches!(language, "js" | "py") {
            "live after GC"
        } else {
            "in its process footprint"
        };
        if let Some(previous) = self.restarted.take() {
            report["recycled"] = true.into();
            report["notice"] = format!("[{language} kernel was restarted before this cell: {} {measure} exceeded the {} ceiling. Every global from earlier cells is gone.{}]", bytes(previous["liveBytes"].as_u64().unwrap_or(0)), bytes(ceiling), globals(&previous, "Largest globals were")).into();
        } else if !matches!(language, "js" | "py") || report["gcRan"] == true {
            if ceiling > 0 && live >= ceiling {
                report["overCeiling"] = true.into();
                if self.pending.is_none() {
                    report["notice"] = format!("[{language} kernel holds {} {measure}, over the {} ceiling. It restarts before the next cell you submit and every global is lost.{}]", bytes(live), bytes(ceiling), globals(report, "Largest globals")).into();
                    self.pending = Some(report.clone());
                }
                self.notified = live;
            } else if notice > 0
                && live >= notice
                && (crossed || live >= self.notified.saturating_mul(5) / 4)
            {
                self.notified = live;
                let name = report["globals"][0]["name"].as_str().unwrap_or("name");
                let drop = match language {
                    "py" => format!("del {name}"),
                    "rb" => format!("{name} = nil"),
                    "jl" => format!("{name} = nothing"),
                    _ => format!("{name} = undefined"),
                };
                report["notice"] = format!("[{language} kernel holds {} {measure} (notice at {}).{} Drop what you no longer need ({drop}) or run with reset: true.]", bytes(live), bytes(notice), globals(report, "Largest globals")).into();
            }
        }
    }

    pub fn recycle(&mut self) {
        self.restarted = self.pending.take();
        self.notified = 0;
        self.observed = 0;
    }
}

fn bytes(value: u64) -> String {
    if value >= 1024 * 1024 * 1024 {
        let tenths =
            value.saturating_mul(10).saturating_add(512 * 1024 * 1024) / (1024 * 1024 * 1024);
        if tenths % 10 == 0 {
            format!("{} GB", tenths / 10)
        } else {
            format!("{}.{} GB", tenths / 10, tenths % 10)
        }
    } else {
        format!("{} MB", ((value + 512 * 1024) / (1024 * 1024)).max(1))
    }
}

fn globals(report: &Value, label: &str) -> String {
    let listed = report["globals"]
        .as_array()
        .into_iter()
        .flatten()
        .take(5)
        .filter_map(|entry| {
            Some(format!(
                "{} {}{}",
                entry["name"].as_str()?,
                if entry["approximate"] == true {
                    "~"
                } else {
                    ""
                },
                bytes(entry["bytes"].as_u64()?)
            ))
        })
        .collect::<Vec<_>>();
    if listed.is_empty() {
        String::new()
    } else {
        format!(" {label}: {}.", listed.join(", "))
    }
}

pub(crate) async fn footprint(pid: Option<u32>) -> Option<Value> {
    let pid = pid?;
    #[cfg(target_os = "linux")]
    {
        let status = tokio::fs::read_to_string(format!("/proc/{pid}/status"))
            .await
            .ok()?;
        let rss = status
            .lines()
            .find_map(|line| line.strip_prefix("RssAnon:"))?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?;
        Some(json!({"liveBytes":rss * 1024,"measure":"footprint"}))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let output = tokio::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .await
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let rss = std::str::from_utf8(&output.stdout)
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()?;
        Some(json!({"liveBytes":rss * 1024,"measure":"footprint","approximate":true}))
    }
}

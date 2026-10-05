use crate::MemorySettings;
use deno_core::JsRuntime;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub(super) struct Memory {
    settings: MemorySettings,
    last_live: usize,
    noticed: usize,
    last_collection: Instant,
    collection_cost: Duration,
    idle_pending: bool,
}

impl Memory {
    pub fn new(settings: MemorySettings) -> Self {
        Self {
            settings,
            last_live: 0,
            noticed: 0,
            last_collection: Instant::now(),
            collection_cost: Duration::ZERO,
            idle_pending: false,
        }
    }

    fn estimate(runtime: &mut JsRuntime) -> usize {
        let stats = runtime.v8_isolate().get_heap_statistics();
        stats
            .used_heap_size()
            .saturating_add(stats.external_memory())
    }

    pub fn collect(&mut self, runtime: &mut JsRuntime) -> usize {
        let start = Instant::now();
        runtime.v8_isolate().low_memory_notification();
        runtime.v8_isolate().low_memory_notification();
        self.collection_cost = start.elapsed();
        self.last_collection = Instant::now();
        self.last_live = Self::estimate(runtime);
        self.idle_pending = false;
        if self.last_live < self.settings.notice_mb * 1024 * 1024 / 2 {
            self.noticed = 0;
        }
        self.last_live
    }

    pub fn idle_delay(&self) -> Option<Duration> {
        (self.idle_pending
            && self.settings.gc_watermark_mb > 0
            && self.last_live >= self.settings.gc_watermark_mb * 1024 * 1024)
            .then(|| {
                Duration::from_secs(1)
                    .max((self.collection_cost * 20).saturating_sub(self.last_collection.elapsed()))
            })
    }

    pub fn after_cell(&mut self, runtime: &mut JsRuntime) -> Value {
        let estimate = Self::estimate(runtime);
        let reached = |limit: usize| limit > 0 && estimate >= limit.saturating_mul(1024 * 1024);
        let collect = reached(self.settings.ceiling_mb)
            || reached(self.settings.gc_watermark_mb)
                && estimate
                    > self
                        .last_live
                        .saturating_add((64 * 1024 * 1024).max(self.last_live / 4))
            || reached(self.settings.notice_mb)
                && (self.last_live < self.settings.notice_mb * 1024 * 1024
                    || self.noticed == 0
                    || estimate >= self.noticed.saturating_mul(5) / 4);
        let live = if collect {
            self.collect(runtime)
        } else {
            estimate
        };
        self.idle_pending = true;
        let mut report = json!({"liveBytes":live,"measure":"heap"});
        if collect {
            report["gcRan"] = true.into();
            if self.settings.notice_mb > 0 && live >= self.settings.notice_mb * 1024 * 1024 {
                self.noticed = live;
            }
            if (self.settings.notice_mb > 0 && live >= self.settings.notice_mb * 1024 * 1024)
                || (self.settings.ceiling_mb > 0 && live >= self.settings.ceiling_mb * 1024 * 1024)
            {
                report["globals"] = globals(runtime).unwrap_or_else(|| json!([]));
            }
        }
        report
    }
}

fn globals(runtime: &mut JsRuntime) -> Option<Value> {
    let value = runtime
        .execute_script("harness-eval-memory.js", "__harness_globals()")
        .ok()?;
    deno_core::scope!(scope, runtime);
    let value = deno_core::v8::Local::new(scope, value);
    deno_core::serde_v8::from_v8(scope, value).ok()
}

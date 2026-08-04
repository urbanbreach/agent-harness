use harness_tui::perf_budgets::*;

fn frame(total_frame_us: u64, phase: FramePhase) -> FrameMetrics {
    FrameMetrics {
        input_to_render_us: 1,
        render_to_flush_us: total_frame_us - 1,
        total_frame_us,
        phase,
    }
}

#[test]
fn frame_metrics_enforce_timing_gates() {
    let metrics = frame(16_000, FramePhase::Render);
    assert!(metrics.meets_target_fps(60));
    assert!(metrics.within_110pct_gate(15_000));
    assert!(metrics.cadence_gap_ok(frame(32_000, FramePhase::Flush)));
}

#[test]
fn frame_clock_tracks_percentiles_and_redraws() {
    let mut clock = FrameClock::new(4);
    for total in [10, 20, 30, 40] {
        clock.record(frame(total, FramePhase::Render));
    }
    assert_eq!(clock.p95_total_us(), 40);
    assert_eq!(clock.max_cadence_gap(), 200);
    assert_eq!(clock.settled_redraw_count(), 0);
}

#[test]
fn queue_bounds_choose_backpressure() {
    let bounds = QueueBounds::defaults();
    assert_eq!(bounds.decide(2, 0), BackpressureDecision::Drop);
    assert_eq!(bounds.decide(0, 205), BackpressureDecision::Throttle(16));
    assert_eq!(bounds.decide(0, 1), BackpressureDecision::Accept);
    assert!(bounds.is_within_bounds(2, 256, 2));
    assert!(!bounds.is_within_bounds(3, 256, 2));
}

#[test]
fn resource_budget_enforces_cache_and_workers() {
    let budget = ResourceBudget::defaults();
    let empty = ResourceSnapshot::new();
    assert!(budget.is_within_budget(&empty));
    let mut workers = empty;
    workers.active_workers = 3;
    assert!(!budget.is_within_budget(&workers));
    let mut cache = empty;
    cache.cache_bytes = budget.image_bounds().max_bytes
        + budget.mermaid_bounds().max_bytes
        + budget.transcript_bounds().max_bytes
        + 1;
    assert!(!budget.is_within_budget(&cache));
    let mut before = empty;
    before.active_workers = 2;
    assert!(budget.prompt_cancellation_invariant(&before, &empty));
}

#[test]
fn resource_snapshots_detect_sustained_growth() {
    let samples = [0, 600_000, 1_200_000, 1_700_000].map(|cache_bytes| ResourceSnapshot {
        cache_bytes,
        ..ResourceSnapshot::new()
    });
    assert!(samples[3].has_sustained_growth(&samples));
    let decreasing = [samples[0], samples[2], samples[1]];
    assert!(!decreasing[2].has_sustained_growth(&decreasing));
}

#[test]
fn sample_window_enforces_stress_budgets() {
    let mut window = SampleWindow::new(100);
    for tick in 0..100 {
        let resources = ResourceSnapshot {
            cache_bytes: tick * 20_000,
            ..ResourceSnapshot::new()
        };
        window.record(StressSample {
            tick: tick as u64,
            frame_metrics: frame(16_000, FramePhase::Render),
            resources,
        });
    }
    assert_eq!(window.len(), 100);
    assert!(window.p95_frame_budget_gate(16_667));
    assert!(window.max_cadence_gap_ratio() <= 200);
    assert_eq!(window.settled_redraw_count(), 0);
    assert!(window.memory_growth_over_window());
}

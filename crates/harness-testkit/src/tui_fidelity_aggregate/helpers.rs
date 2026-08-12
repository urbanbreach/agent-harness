use super::*;

pub(super) fn summarize(
    authority: Authority,
    runs: &[Run],
) -> Result<AggregateSummary, AggregateError> {
    let reference = collect(runs, |metrics| {
        &metrics
            .reference
            .external_send_to_changed_observation_micros
    });
    let candidate = collect(runs, |metrics| {
        &metrics
            .candidate
            .external_send_to_changed_observation_micros
    });
    let intervals = collect(runs, |metrics| {
        &metrics.candidate.external_observation_intervals_micros
    });
    let native_receive = collect(runs, |metrics| {
        metrics.candidate.native.as_ref().map_or(&[], |native| {
            native.receive_to_successful_flush_micros.as_slice()
        })
    });
    let reference_p95 = p95(&reference)?;
    let candidate_p95 = p95(&candidate)?;
    if candidate_p95.saturating_mul(100) > reference_p95.saturating_mul(110) {
        return Err(AggregateError::Threshold(
            "external p95 exceeds 110%".into(),
        ));
    }
    for run in runs {
        check_gap(&run.metrics.reference)?;
        check_gap(&run.metrics.candidate)?;
    }
    let native = runs
        .iter()
        .filter_map(|run| run.metrics.candidate.native.as_ref());
    let (mut coalesced, mut saturation, mut resyncs, mut repaints, mut idle) = (0, 0, 0, 0, 0);
    for metrics in native {
        coalesced += metrics.coalesced_requests;
        saturation += metrics.queue_saturation;
        resyncs += metrics.resyncs;
        repaints += metrics.full_repaints;
        idle += metrics.idle_redraws;
    }
    if idle != 0 {
        return Err(AggregateError::Threshold(
            "idle redraws must be zero".into(),
        ));
    }
    Ok(AggregateSummary {
        schema_version: "harness.tui-fidelity.aggregate.v1",
        run_count: runs.len(),
        authority,
        reference_external_p95_micros: reference_p95,
        candidate_external_p95_micros: candidate_p95,
        candidate_native_receive_to_flush_p95_micros: p95(&native_receive)?,
        candidate_interval_p95_micros: p95(&intervals)?,
        candidate_interval_max_micros: intervals.iter().copied().max().unwrap_or_default(),
        coalesced_requests: coalesced,
        queue_saturation: saturation,
        resyncs,
        full_repaints: repaints,
        idle_redraws: idle,
        artifact_sha256: runs.iter().flat_map(|run| run.artifacts.clone()).collect(),
    })
}

fn collect(
    runs: &[Run],
    values: impl Fn(&crate::tui_fidelity_compare::PresentationComparisonMetrics) -> &[u64],
) -> Vec<u64> {
    runs.iter()
        .flat_map(|run| values(&run.metrics).iter().copied())
        .collect()
}

fn p95(values: &[u64]) -> Result<u64, AggregateError> {
    let mut values = values.to_vec();
    values.sort_unstable();
    let rank = (values.len().saturating_mul(95).saturating_add(99)) / 100;
    values
        .get(rank.saturating_sub(1))
        .copied()
        .ok_or_else(|| AggregateError::Threshold("empty aggregate metric".into()))
}

fn check_gap(metrics: &PresentationTimingMetrics) -> Result<(), AggregateError> {
    if metrics.external_cadence_micros > 0
        && metrics
            .external_observation_intervals_micros
            .iter()
            .any(|gap| *gap > metrics.external_cadence_micros.saturating_mul(2))
    {
        return Err(AggregateError::Threshold(
            "gap exceeds twice cadence".into(),
        ));
    }
    Ok(())
}

pub(super) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, AggregateError> {
    let bytes = std::fs::read(path).map_err(|error| AggregateError::Evidence {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    serde_json::from_slice(&bytes).map_err(|error| AggregateError::Evidence {
        path: path.to_path_buf(),
        detail: format!("invalid JSON: {error}"),
    })
}

pub(super) fn find_unique(root: &Path, name: &str) -> Result<PathBuf, AggregateError> {
    let direct = root.join(name);
    if direct.is_file() {
        return Ok(direct);
    }
    let mut matches = Vec::new();
    visit(root, name, &mut matches)?;
    if matches.len() != 1 {
        return evidence(
            root,
            &format!("expected exactly one {name}, got {}", matches.len()),
        );
    }
    Ok(matches.remove(0))
}

fn visit(root: &Path, name: &str, matches: &mut Vec<PathBuf>) -> Result<(), AggregateError> {
    let entries = std::fs::read_dir(root).map_err(|error| AggregateError::Evidence {
        path: root.to_path_buf(),
        detail: error.to_string(),
    })?;
    for entry in entries {
        let path = entry
            .map_err(|error| AggregateError::Evidence {
                path: root.to_path_buf(),
                detail: error.to_string(),
            })?
            .path();
        if path.is_dir() {
            visit(&path, name, matches)?;
        } else if path.file_name().is_some_and(|value| value == name) {
            matches.push(path);
        }
    }
    Ok(())
}

pub(super) fn verify_artifact(artifact: &Artifact) -> Result<(), AggregateError> {
    let bytes = std::fs::read(&artifact.path).map_err(|error| AggregateError::Evidence {
        path: artifact.path.clone(),
        detail: error.to_string(),
    })?;
    let actual =
        Sha256::digest(bytes)
            .iter()
            .fold(String::with_capacity(64), |mut output, byte| {
                use std::fmt::Write as _;
                let _ = write!(output, "{byte:02x}");
                output
            });
    if actual != artifact.sha256 {
        return evidence(&artifact.path, "stale artifact digest");
    }
    Ok(())
}

pub(super) fn evidence<T>(path: &Path, detail: &str) -> Result<T, AggregateError> {
    Err(AggregateError::Evidence {
        path: path.to_path_buf(),
        detail: detail.to_owned(),
    })
}

use std::fs;
use std::path::{Path, PathBuf};

const EVENTS_FILE_NAME: &str = "events.jsonl";
const WRITER_LOCK_FILE_NAME: &str = ".writer.lock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TrashSessionReport {
    pub(super) run_id: String,
    pub(super) source_run_dir: PathBuf,
    pub(super) trash_run_dir: PathBuf,
}

pub(super) fn trash_session_run_dir_for_tui(
    run_id: &str,
    run_dir: &Path,
    configured_session_dir: Option<&Path>,
    current_run_dir: Option<&Path>,
) -> Result<TrashSessionReport, String> {
    if run_id.trim().is_empty() {
        return Err("session delete failed: run id is empty".to_string());
    }
    let source_run_dir = canonical_existing_dir(run_dir, "session run directory")?;
    if !source_run_dir.join(EVENTS_FILE_NAME).is_file() {
        return Err(format!(
            "session delete failed: {} is not a Harness run directory",
            source_run_dir.display()
        ));
    }
    if source_run_dir.join(WRITER_LOCK_FILE_NAME).exists() {
        return Err(format!(
            "session delete refused: source run is actively writer-locked: {}",
            source_run_dir.display()
        ));
    }

    if let Some(current_run_dir) = current_run_dir {
        let current_run_dir = canonical_existing_dir(current_run_dir, "current run directory")?;
        if current_run_dir == source_run_dir {
            return Err(format!(
                "session delete refused: cannot delete current run {}",
                source_run_dir.display()
            ));
        }
    }

    let session_dir = session_root_for_run(&source_run_dir, configured_session_dir)?;
    let source_parent = source_run_dir
        .parent()
        .ok_or_else(|| "session delete failed: run directory has no parent".to_string())?
        .canonicalize()
        .map_err(|err| format!("session delete failed: canonicalize parent: {err}"))?;
    if source_parent != session_dir {
        return Err(format!(
            "session delete refused: {} is outside session root {}",
            source_run_dir.display(),
            session_dir.display()
        ));
    }

    let trash_root = session_dir.join("trash");
    fs::create_dir_all(&trash_root).map_err(|err| {
        format!(
            "session delete failed: create trash directory {}: {err}",
            trash_root.display()
        )
    })?;
    let trash_run_dir = unused_trash_path(&trash_root, &source_run_dir)?;
    fs::rename(&source_run_dir, &trash_run_dir).map_err(|err| {
        format!(
            "session delete failed: move {} to {}: {err}",
            source_run_dir.display(),
            trash_run_dir.display()
        )
    })?;

    Ok(TrashSessionReport {
        run_id: run_id.to_string(),
        source_run_dir,
        trash_run_dir,
    })
}

fn canonical_existing_dir(path: &Path, label: &str) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|err| format!("session delete failed: canonicalize {label}: {err}"))?;
    if !canonical.is_dir() {
        return Err(format!(
            "session delete failed: {label} is not a directory: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn session_root_for_run(
    source_run_dir: &Path,
    configured_session_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(session_dir) = configured_session_dir {
        return canonical_existing_dir(session_dir, "session root");
    }
    let parent = source_run_dir
        .parent()
        .ok_or_else(|| "session delete failed: run directory has no parent".to_string())?;
    canonical_existing_dir(parent, "session root")
}

fn unused_trash_path(trash_root: &Path, source_run_dir: &Path) -> Result<PathBuf, String> {
    let run_dir_name = source_run_dir
        .file_name()
        .ok_or_else(|| "session delete failed: run directory has no file name".to_string())?;
    let first = trash_root.join(run_dir_name);
    if !first.exists() {
        return Ok(first);
    }
    let run_dir_name = run_dir_name.to_string_lossy();
    for suffix in 1..=999 {
        let candidate = trash_root.join(format!("{run_dir_name}-{suffix}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "session delete failed: no available trash path for {}",
        source_run_dir.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_run(session_dir: &Path, run_id: &str) -> PathBuf {
        let run_dir = session_dir.join(run_id);
        fs::create_dir_all(&run_dir).expect("create run dir");
        fs::write(run_dir.join(EVENTS_FILE_NAME), "{}\n").expect("write events");
        run_dir
    }

    #[test]
    fn trash_session_run_dir_moves_run_to_session_trash() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source = write_run(temp_dir.path(), "run_delete");

        let report =
            trash_session_run_dir_for_tui("run_delete", &source, Some(temp_dir.path()), None)
                .expect("trash session");

        assert_eq!(report.run_id, "run_delete");
        assert!(!report.source_run_dir.exists());
        assert!(report.trash_run_dir.join(EVENTS_FILE_NAME).is_file());
        assert_eq!(
            report.trash_run_dir.parent(),
            Some(temp_dir.path().join("trash").as_path())
        );
    }

    #[test]
    fn trash_session_run_dir_refuses_outside_configured_session_root() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        let source = write_run(outside.path(), "run_outside");

        let err =
            trash_session_run_dir_for_tui("run_outside", &source, Some(temp_dir.path()), None)
                .expect_err("outside root should fail");

        assert!(err.contains("outside session root"), "{err}");
        assert!(source.exists());
    }

    #[test]
    fn trash_session_run_dir_refuses_locked_and_current_runs() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let locked = write_run(temp_dir.path(), "run_locked");
        fs::write(locked.join(WRITER_LOCK_FILE_NAME), "locked").expect("write lock");
        let current = write_run(temp_dir.path(), "run_current");

        let locked_err =
            trash_session_run_dir_for_tui("run_locked", &locked, Some(temp_dir.path()), None)
                .expect_err("locked run should fail");
        assert!(locked_err.contains("writer-locked"), "{locked_err}");

        let current_err = trash_session_run_dir_for_tui(
            "run_current",
            &current,
            Some(temp_dir.path()),
            Some(&current),
        )
        .expect_err("current run should fail");
        assert!(
            current_err.contains("cannot delete current run"),
            "{current_err}"
        );
        assert!(current.exists());
    }
}

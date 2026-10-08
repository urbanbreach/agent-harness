use crate::{CliDeps, CliIo};
use harness_core::{
    config::HarnessConfig,
    redact::{DefaultRedactor, Redactor},
    store,
};
use serde_json::json;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

struct Archive {
    tar: tar::Builder<flate2::write::GzEncoder<tempfile::NamedTempFile>>,
    bytes: u64,
    files: usize,
}
const MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(clap::Args)]
pub(crate) struct TraceCommand {
    session: String,
    #[arg(long, default_value_t = true)]
    local: bool,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}
#[derive(clap::Args)]
pub(crate) struct WrapCommand {
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(long)]
    with_sessions: bool,
}
pub(crate) fn trace(
    command: TraceCommand,
    config: Option<&Path>,
    directory: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let cwd = deps.current_dir().map_err(|e| e.to_string())?;
    let configured = crate::inspect::configured(config, deps)?;
    let redactor = crate::inspect::redactor(&configured.config, deps)?;
    let root = deps.session_directory(
        &directory.unwrap_or_else(|| configured.config.runtime.session_dir.clone()),
    )?;
    let source = crate::recovery::resolve_session_run_dir(&command.session, &root, &cwd)?;
    let id = source
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("invalid session name")?;
    let output = output_path(
        &cwd,
        command
            .output
            .unwrap_or_else(|| format!("{id}.tar.gz").into()),
        source.parent().ok_or("session parent missing")?,
    )?;
    let bytes = create(&output, |archive, staging| {
        add_session(
            archive,
            &source,
            Path::new(""),
            staging,
            &configured.config,
            &redactor,
        )
    })?;
    if command.json {
        crate::inspect::print_json(
            io,
            &json!({"session_id":id,"status":"exported","local_path":output,"bytes":bytes}),
        )
    } else {
        writeln!(io.stdout, "{}", output.display()).map_err(|e| e.to_string())
    }
}
pub(crate) fn wrap(
    command: WrapCommand,
    config: Option<&Path>,
    directory: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let cwd = deps
        .current_dir()
        .and_then(|p| p.canonicalize())
        .map_err(|e| e.to_string())?;
    let configured = crate::inspect::configured(config, deps)?;
    let redactor = crate::inspect::redactor(&configured.config, deps)?;
    let sessions = deps.session_directory(
        &directory.unwrap_or_else(|| configured.config.runtime.session_dir.clone()),
    )?;
    if sessions == cwd || command.with_sessions && !sessions.starts_with(&cwd) {
        return Err("workspace packaging requires a separate session directory; --with-sessions requires it inside the workspace".into());
    }
    let output = output_path(
        &cwd,
        command
            .output
            .unwrap_or_else(|| "workspace.wrap.tar.gz".into()),
        &sessions,
    )?;
    let bytes = create(&output, |archive, staging| {
        workspace_files(
            archive,
            &cwd,
            &[output.clone(), staging.to_owned(), sessions.clone()],
            &redactor,
        )?;
        if command.with_sessions {
            for session in crate::replay::inspect_session_catalog(&sessions)? {
                let prefix = session
                    .run_dir
                    .strip_prefix(&cwd)
                    .map_err(|e| e.to_string())?;
                add_session(
                    archive,
                    &session.run_dir,
                    prefix,
                    staging,
                    &configured.config,
                    &redactor,
                )?;
            }
        }
        Ok(())
    })?;
    crate::inspect::print_json(
        io,
        &json!({"status":"wrapped","output":output,"bytes":bytes}),
    )
}
fn output_path(cwd: &Path, path: PathBuf, sessions: &Path) -> Result<PathBuf, String> {
    let output = harness_core::tool::resolve_file_path(cwd, &path).map_err(|e| e.to_string())?;
    let sessions =
        harness_core::tool::resolve_file_path(cwd, sessions).map_err(|e| e.to_string())?;
    if output.starts_with(sessions) {
        return Err("archive output must be outside session storage".into());
    }
    Ok(output)
}
fn create(
    output: &Path,
    fill: impl FnOnce(&mut Archive, &Path) -> Result<(), String>,
) -> Result<u64, String> {
    let parent = output.parent().ok_or("archive output has no parent")?;
    store::create_private_dir(parent).map_err(|e| e.to_string())?;
    let temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    let path = temporary.path().to_owned();
    let mut archive = Archive {
        tar: tar::Builder::new(flate2::write::GzEncoder::new(
            temporary,
            flate2::Compression::fast(),
        )),
        bytes: 0,
        files: 0,
    };
    fill(&mut archive, &path)?;
    let temporary = archive
        .tar
        .into_inner()
        .map_err(|e| e.to_string())?
        .finish()
        .map_err(|e| e.to_string())?;
    let bytes = temporary
        .as_file()
        .metadata()
        .map_err(|e| e.to_string())?
        .len();
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary.persist(output).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}
fn add(
    archive: &mut Archive,
    path: &Path,
    data: impl Read,
    length: u64,
    mode: u32,
    redactor: &dyn Redactor,
) -> Result<(), String> {
    let name = path.to_str().ok_or("archive path must be UTF-8")?;
    if path
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || name.chars().any(char::is_control)
        || redactor.redact_text(name) != name
        || DefaultRedactor::default().secret_finding_count(name) != 0
    {
        return Err("archive path secret or safety scan failed".into());
    }
    archive.bytes = archive.bytes.saturating_add(length);
    archive.files += 1;
    if archive.bytes > MAX_BYTES || archive.files > 10_000 {
        return Err("archive exceeds 256 MiB or 10,000 files".into());
    }
    let mut header = tar::Header::new_gnu();
    header.set_size(length);
    header.set_mode(mode);
    header.set_mtime(0);
    header.set_cksum();
    archive
        .tar
        .append_data(&mut header, path, data)
        .map_err(|e| e.to_string())
}
fn workspace_files(
    archive: &mut Archive,
    root: &Path,
    excluded: &[PathBuf],
    redactor: &dyn Redactor,
) -> Result<(), String> {
    let excluded = excluded.to_vec();
    let mut walk = ignore::WalkBuilder::new(root);
    walk.hidden(false)
        .parents(false)
        .git_global(false)
        .require_git(false)
        .sort_by_file_path(Path::cmp)
        .filter_entry(move |entry| {
            entry.file_name() != ".git"
                && entry.file_name() != ".jj"
                && entry.file_name() != ".hg"
                && !excluded.iter().any(|path| entry.path().starts_with(path))
        });
    for entry in walk.build() {
        let entry = entry.map_err(|e| e.to_string())?;
        if let Some(error) = entry.error() {
            return Err(error.to_string());
        }
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            continue;
        }
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            return Err("workspace archive contains a symlink or unsupported file".into());
        }
        workspace_file(archive, root, entry.path(), redactor)?;
    }
    Ok(())
}
fn workspace_file(
    archive: &mut Archive,
    root: &Path,
    path: &Path,
    redactor: &dyn Redactor,
) -> Result<(), String> {
    let file = store::open_private_file(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if metadata.len() > 16 * 1024 * 1024 {
        return Err("workspace archive file exceeds 16 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("workspace archive file exceeds 16 MiB".into());
    }
    let text = String::from_utf8_lossy(&bytes);
    if redactor.redact_text(&text) != text
        || DefaultRedactor::default().secret_finding_count(&text) != 0
    {
        return Err("workspace archive secret scan failed".into());
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    };
    #[cfg(not(unix))]
    let mode = 0o644;
    add(
        archive,
        path.strip_prefix(root).map_err(|e| e.to_string())?,
        bytes.as_slice(),
        bytes.len() as u64,
        mode,
        redactor,
    )
}
fn add_session(
    archive: &mut Archive,
    source: &Path,
    prefix: &Path,
    staging: &Path,
    config: &HarnessConfig,
    redactor: &dyn Redactor,
) -> Result<(), String> {
    let parent = staging.parent().ok_or("archive staging parent missing")?;
    let temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| e.to_string())?
        .into_temp_path();
    crate::exports::journal(&source.join("events.jsonl"), &temporary, redactor)?;
    let mut journal = store::open_private_file(&temporary).map_err(|e| e.to_string())?;
    let length = journal.metadata().map_err(|e| e.to_string())?.len();
    add(
        archive,
        &prefix.join("events.jsonl"),
        &mut journal,
        length,
        0o600,
        redactor,
    )?;
    let mut support = crate::exports::bundle(source, config, false)?;
    support["support"]["raw_artifacts_omitted"] = true.into();
    for (name, value) in [
        ("meta.json", support["metadata"].clone()),
        ("support.json", support),
    ] {
        let bytes = crate::exports::checked_json(value, redactor)?;
        add(
            archive,
            &prefix.join(name),
            bytes.as_slice(),
            bytes.len() as u64,
            0o600,
            redactor,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_limit_leaves_previous_archive_intact() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let output = directory.path().join("archive.tar.gz");
        fs::write(&output, "previous archive")?;
        let bytes = vec![0; 64 * 1024 * 1024];
        let result = create(&output, |archive, _| {
            for i in 0..5 {
                add(
                    archive,
                    Path::new(&format!("session-{i}/events.jsonl")),
                    bytes.as_slice(),
                    bytes.len() as u64,
                    0o600,
                    &DefaultRedactor::default(),
                )?;
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&output)?, b"previous archive");
        Ok(())
    }
}

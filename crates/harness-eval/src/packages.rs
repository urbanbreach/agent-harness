//! Session-local, transactional package installs. No shell expansion or project writes.
use crate::{cell::Cell, session::Inner, Result};
use serde_json::{json, Value};
use std::{path::Path, process::Stdio};
use tokio::{io::AsyncReadExt, process::Command};

pub(crate) async fn install(session: &Inner, cell: &Cell, args: &Value) -> Result<Value> {
    let manager = args["manager"]
        .as_str()
        .ok_or("package manager is required")?;
    if !matches!(
        (manager, cell.language.as_str()),
        ("pip", "py") | ("npm", "js")
    ) {
        return Err("use %pip install in Python or %npm install in JavaScript".into());
    }
    let arguments = args["arguments"]
        .as_str()
        .ok_or("package arguments are required")?;
    if arguments.len() > 8192 || arguments.contains(['\n', '\r', '\0']) {
        return Err("invalid package arguments".into());
    }
    let mut packages = shell_words::split(arguments)?;
    if packages.is_empty()
        || packages.len() > 64
        || packages.iter().any(|p| p.is_empty() || p.starts_with('-'))
    {
        return Err("install accepts 1-64 package specifications, without installer flags".into());
    }
    for package in &mut packages {
        let local = session.options.cwd.join(&*package);
        if local.exists() {
            *package = local.canonicalize()?.to_string_lossy().into_owned();
        }
    }
    let root = session
        .options
        .local_dir
        .join("environments")
        .join(&cell.language);
    std::fs::create_dir_all(&root)?;
    let staged = tempfile::Builder::new()
        .prefix("revision-")
        .tempdir_in(&root)?;
    let current = root.join("current.json");
    if current.exists() {
        let previous: std::path::PathBuf = serde_json::from_slice(&std::fs::read(&current)?)?;
        // ponytail: copy the last revision; optimize only if large environments become a measured cost.
        copy_tree(&previous, staged.path())?;
    }
    let mut command = if manager == "pip" {
        let mut command = Command::new(crate::kernel::interpreter(
            &session.options,
            &["python3", "python"],
        )?);
        command.args([
            "-m",
            "pip",
            "--isolated",
            "install",
            "--disable-pip-version-check",
            "--no-input",
            "--no-compile",
            "--upgrade",
            "--target",
        ]);
        command.arg(staged.path());
        command
    } else {
        let mut command = Command::new(crate::kernel::interpreter(&session.options, &["npm"])?);
        if !staged.path().join("package.json").exists() {
            std::fs::write(staged.path().join("package.json"), b"{\"private\":true}")?;
        }
        command.args([
            "install",
            "--ignore-scripts",
            "--install-links",
            "--no-audit",
            "--no-fund",
            "--prefix",
        ]);
        command.arg(staged.path());
        command
    };
    let cache = tempfile::Builder::new()
        .prefix("installer-")
        .tempdir_in(&root)?;
    let empty_config = cache.path().join("empty-config");
    std::fs::write(&empty_config, "")?;
    let global_config = cache.path().join("global-config");
    std::fs::write(&global_config, "")?;
    command
        .args(&packages)
        .current_dir(staged.path())
        .env_clear()
        .envs(session.options.environment.iter().filter(|(key, _)| {
            let key = key.to_string_lossy().to_ascii_uppercase();
            !key.starts_with("NPM_CONFIG_")
                && !key.starts_with("PIP_")
                && !key.starts_with("PYTHON")
                && key != "NODE_OPTIONS"
        }))
        .env("PIP_CONFIG_FILE", &empty_config)
        .env("NPM_CONFIG_USERCONFIG", &empty_config)
        .env("NPM_CONFIG_GLOBALCONFIG", &global_config)
        .env("NPM_CONFIG_CACHE", cache.path().join("cache"))
        .env("PIP_CACHE_DIR", cache.path().join("cache"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = command.spawn()?;
    let _group = ProcessGroup(child.id());
    let stdout = child.stdout.take().ok_or("installer stdout missing")?;
    let stderr = child.stderr.take().ok_or("installer stderr missing")?;
    let mut state = cell.state.lock().await;
    if state.busy == 0 {
        state.blocked_since = Some(tokio::time::Instant::now());
    }
    state.busy += 1;
    drop(state);
    let result = tokio::select! {
        () = cell.cancel.cancelled() => Err("package installation cancelled".into()),
        result = async {
            let (status, stdout, stderr) = tokio::join!(child.wait(), tail(stdout), tail(stderr));
            Ok::<_, crate::Error>((status?, stdout?, stderr?))
        } => result,
    };
    let mut state = cell.state.lock().await;
    state.busy -= 1;
    if state.busy == 0
        && let Some(started) = state.blocked_since.take()
    {
        state.blocked_time += started.elapsed();
    }
    drop(state);
    let (status, stdout, stderr) = result?;
    if !status.success() || cell.cancel.is_cancelled() {
        return Err(format!(
            "{manager} install failed ({status}); previous environment retained\n{stdout}{stderr}"
        )
        .into());
    }
    // Publishing one pointer keeps old imports valid and failed/cancelled revisions invisible.
    let path = staged.path().to_owned();
    let mut pointer = tempfile::NamedTempFile::new_in(&root)?;
    serde_json::to_writer(pointer.as_file_mut(), &path)?;
    pointer.persist(&current)?;
    let _published = staged.keep();
    Ok(
        json!({"path":path,"manager":manager,"text":format!("Installed into managed {manager} environment.\n{stdout}{stderr}")}),
    )
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            // npm's .bin links are recreated by install; reject other linked contents.
            if source.file_name().is_some_and(|name| name == ".bin") {
                continue;
            }
            return Err("managed package revisions must not contain symbolic links".into());
        }
        if kind.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)?;
        } else {
            return Err("managed package revisions must contain only regular files".into());
        }
    }
    Ok(())
}

async fn tail(mut stream: impl tokio::io::AsyncRead + Unpin) -> Result<String> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            break;
        }
        output.extend_from_slice(&chunk[..count]);
        output.drain(..output.len().saturating_sub(8192));
    }
    Ok(String::from_utf8_lossy(&output).into_owned())
}

struct ProcessGroup(Option<u32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self
            .0
            .and_then(|pid| i32::try_from(pid).ok())
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }
}

use harness_core::{
    sandbox::*,
    tool::{ToolContext, ToolError},
};
use std::{ffi::OsString, path::Path};
use tokio::process::Command;

const ALL: &str = "execute,write-file,read-file,read-dir,remove-dir,remove-file,make-char,make-dir,make-reg,make-sock,make-fifo,make-block,make-sym,refer,truncate,ioctl-dev";
const READ: &str = "execute,read-file,read-dir";
const FILE_WRITE: &str = "execute,read-file,write-file,truncate,ioctl-dev";

pub(super) fn prepare(
    ctx: &ToolContext,
    policy: SandboxPolicy,
) -> Result<(Command, Option<tempfile::TempDir>), ToolError> {
    #[cfg(target_os = "linux")]
    rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
        .map_err(|_| {
            ToolError::Execution("cannot protect the parent process environment".into())
        })?;
    if policy == SandboxPolicy::Off {
        return Ok((Command::new("bash"), None));
    }
    if !cfg!(target_os = "linux") || !detect_landlock().is_available() {
        return Err(ToolError::Execution(
            "OS sandbox policy requires Linux Landlock filesystem ABI 5".into(),
        ));
    }
    let scratch = tempfile::Builder::new()
        .prefix("harness-shell-")
        .tempdir()?;
    let roots = SandboxPathRoots {
        workspace_root: ctx.workspace_root.clone(),
        harness_state_dir: ctx
            .artifacts_dir
            .parent()
            .ok_or_else(|| ToolError::Execution("session directory unavailable".into()))?
            .canonicalize()?,
        temp_dir: scratch.path().canonicalize()?,
    };
    let plan = build_fs_plan(policy, &roots)
        .ok_or_else(|| ToolError::Execution("invalid OS sandbox filesystem roots".into()))?;
    // setpriv applies Landlock after exec. No unsafe fork hook or extra resident process.
    let mut command = Command::new("/usr/bin/setpriv");
    command.args(["--no-new-privs", "--landlock-access", &format!("fs:{ALL}")]);
    for path in &plan.read_roots {
        rule(
            &mut command,
            path,
            if path.is_dir() {
                READ
            } else {
                "execute,read-file"
            },
        );
    }
    for path in &plan.write_roots {
        rule(
            &mut command,
            path,
            if path.is_dir() { ALL } else { FILE_WRITE },
        );
    }
    command.args(["--", "/bin/bash"]);
    Ok((command, Some(scratch)))
}
fn rule(command: &mut Command, path: &Path, rights: &str) {
    let mut value = OsString::from(format!("path-beneath:{rights}:"));
    value.push(path);
    command.arg("--landlock-rule").arg(value);
}
pub(super) fn environment(command: &mut Command, scratch: Option<&Path>) {
    harness_core::process::environment(command);
    if let Some(scratch) = scratch {
        for key in ["TMPDIR", "TEMP", "TMP"] {
            command.env(key, scratch);
        }
    }
}

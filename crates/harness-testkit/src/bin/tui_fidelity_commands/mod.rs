use std::ffi::OsString;
use std::path::Path;

mod closure;
mod matrix;
mod packet6;
mod task_gate;
mod verify;
mod verify_executor;

pub fn execute(arguments: Vec<OsString>, repo_root: &Path) -> Result<(), String> {
    match arguments.first().and_then(|value| value.to_str()) {
        Some("matrix") => matrix::execute(arguments, repo_root),
        Some("packet6-capability") => packet6::execute(arguments),
        Some("verify") => verify::execute(arguments, repo_root),
        Some("task-admit") => task_gate::execute_admit(arguments, repo_root),
        Some("task-verify") => task_gate::execute_verify(arguments, repo_root),
        Some("task-complete") => task_gate::execute_complete(arguments, repo_root),
        Some("closure-verify") => closure::execute_verify(arguments, repo_root),
        Some("closure-complete") => closure::execute_complete(arguments),
        Some(command) => Err(format!("unknown tui-fidelity command {command}")),
        None => Err("missing tui-fidelity command".to_owned()),
    }
}

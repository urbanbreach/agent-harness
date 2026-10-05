use std::path::Path;
use tokio::process::Command;

pub(super) fn command(executable: &Path, directory: &Path) -> crate::Result<Command> {
    for (name, source) in [
        ("worker.cjs", include_str!("javascript/worker.cjs")),
        ("runtime.cjs", include_str!("javascript/runtime.cjs")),
        ("transform.cjs", include_str!("javascript/transform.cjs")),
        ("memory.cjs", include_str!("javascript/memory.cjs")),
        ("stdio.cjs", include_str!("javascript/stdio.cjs")),
        ("tools.js", include_str!("javascript/tools.js")),
        ("prelude.js", include_str!("javascript/prelude.js")),
        ("acorn.cjs", include_str!("../vendor/acorn.cjs")),
        ("ACORN-LICENSE", include_str!("../vendor/ACORN-LICENSE")),
    ] {
        std::fs::write(directory.join(name), source)?;
    }
    let mut command = Command::new(executable);
    command.arg(directory.join("worker.cjs"));
    Ok(command)
}

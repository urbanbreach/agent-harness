use crate::{CliDeps, CliIo};
use harness_core::worktree::{
    list_session_worktrees, remove_session_worktree, ListedWorktree, RemoveWorktreeOptions,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub(crate) struct WorktreeCommand {
    #[arg(long, global = true)]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    action: Action,
}
#[derive(clap::Subcommand)]
enum Action {
    List {
        #[arg(long)]
        all: bool,
    },
    Remove {
        slug: String,
        #[command(flatten)]
        options: Removal,
    },
    Cleanup(Removal),
}
#[derive(clap::Args)]
struct Removal {
    #[arg(long)]
    force: bool,
    #[arg(long)]
    keep_branch: bool,
}
pub(crate) fn execute(
    command: WorktreeCommand,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let root = crate::workspace::workspace_root(command.workspace, deps)?;
    let data_dir = deps.data_directory()?;
    let mut entries = list_session_worktrees(&root, None, &data_dir).map_err(|e| e.to_string())?;
    let (mut report, failed) = match command.action {
        Action::List { all } => {
            let count = entries.iter().filter(|entry| entry.harness_managed).count();
            entries.retain(|entry| all || entry.harness_managed);
            (
                json!({"repository_root":root,"managed_count":count,"worktrees":entries}),
                false,
            )
        }
        Action::Remove { slug, options } => {
            let entry = entries
                .iter()
                .find(|entry| entry.harness_managed && entry.slug.as_deref() == Some(&slug))
                .ok_or("managed worktree slug was not found")?;
            (remove(&root, entry, &options, &data_dir)?, false)
        }
        Action::Cleanup(options) => {
            let (mut removed, mut failed) = (Vec::new(), Vec::new());
            for entry in entries.iter().filter(|entry| entry.harness_managed) {
                match remove(&root, entry, &options, &data_dir) {
                    Ok(report) => removed.push(report),
                    Err(error) => {
                        failed.push(json!({"slug":entry.slug,"path":entry.path,"error":error}))
                    }
                }
            }
            let incomplete = !failed.is_empty();
            (
                json!({"repository_root":root,"removed_count":removed.len(),"failed_count":failed.len(),
                "removed":removed,"failed":failed}),
                incomplete,
            )
        }
    };
    harness_core::redact::redact_in_place(
        &harness_core::redact::DefaultRedactor::default(),
        &mut report,
    );
    crate::inspect::print_json(io, &report)?;
    if failed {
        Err("some worktrees could not be removed; inspect the report".into())
    } else {
        Ok(())
    }
}
fn remove(
    root: &Path,
    entry: &ListedWorktree,
    options: &Removal,
    data_dir: &Path,
) -> Result<Value, String> {
    remove_session_worktree(RemoveWorktreeOptions {
        repository_root: root,
        data_dir,
        path: &entry.path,
        worktree_parent: None,
        delete_branch: !options.keep_branch,
        force: options.force,
    })
    .map_err(|e| e.to_string())?;
    Ok(
        json!({"removed":true,"slug":entry.slug,"path":entry.path,"branch":entry.branch,"delete_branch":!options.keep_branch}),
    )
}

//! Loads directory-scoped project instructions after successful filesystem tools.
use super::{reminders::PendingReminder, runtime::Job, runtime::JobKind, *};
use crate::perm::PermissionAction;
use std::{collections::BTreeSet, fs::File, io::Read};

pub(super) struct Discovery {
    paths: Vec<PathBuf>,
    roots: Vec<PathBuf>,
    workspace_root: PathBuf,
    startup: Vec<PathBuf>,
    seen: BTreeSet<PathBuf>,
    policy: PermissionPolicy,
    agent_policy: PermissionPolicy,
    max_bytes: u32,
}

pub(super) struct Instruction {
    path: PathBuf,
    body: String,
}

impl Runtime {
    /// Captures authority on the actor; filesystem discovery runs on the blocking pool.
    pub(super) fn directory_instruction_discovery(&self, job: &Job) -> Option<Discovery> {
        let settings = &self.config.behavior.directory_instructions;
        let JobKind::Tool { paths, .. } = &job.kind else {
            return None;
        };
        let agent = self.agents.get(job.actor.agent_id.as_ref()?)?;
        if !settings.enabled || paths.is_empty() || !agent.busy {
            return None;
        }
        Some(Discovery {
            paths: paths.clone(),
            roots: agent
                .execution
                .policy_roots
                .iter()
                .map(PathBuf::from)
                .collect(),
            workspace_root: self.info.as_ref()?.workspace_root.clone(),
            startup: self.config.instruction_paths.clone(),
            seen: self
                .instructions_seen
                .get(&agent.info.agent_id)
                .cloned()
                .unwrap_or_default(),
            policy: self.config.permission_policy.clone(),
            agent_policy: agent.policy.clone(),
            max_bytes: settings.max_bytes,
        })
    }

    pub(super) fn queue_directory_instructions(
        &mut self,
        job: &Job,
        instructions: Vec<Instruction>,
    ) {
        let Some(agent) = job.actor.agent_id.as_ref() else {
            return;
        };
        for Instruction { path, body } in instructions {
            if self
                .instructions_seen
                .get(agent)
                .is_some_and(|seen| seen.contains(&path))
            {
                continue;
            }
            if self.queue_reminder(
                agent,
                PendingReminder {
                    kind: RuntimeReminderKind::DirectoryInstructions,
                    body,
                    source: Some(path.to_string_lossy().into_owned()),
                },
            ) {
                self.instructions_seen
                    .entry(agent.clone())
                    .or_default()
                    .insert(path);
            }
        }
    }
}

impl Discovery {
    pub(super) fn read(self) -> Vec<Instruction> {
        let roots: Vec<_> = self
            .roots
            .iter()
            .filter_map(|root| root.canonicalize().ok())
            .collect();
        let mut startup = BTreeSet::new();
        for path in &self.startup {
            let original = self.workspace_root.join(path);
            let canonical = original.canonicalize().ok();
            if let Ok(relative) = canonical
                .as_deref()
                .unwrap_or(&original)
                .strip_prefix(&self.workspace_root)
            {
                startup.extend(
                    roots
                        .iter()
                        .filter_map(|root| root.join(relative).canonicalize().ok()),
                );
            }
            if let Some(canonical) = canonical {
                startup.insert(canonical);
            }
        }
        let mut directories = BTreeSet::new();
        for path in &self.paths {
            let Some(root) = roots
                .iter()
                .filter(|root| path.starts_with(root))
                .max_by_key(|root| root.components().count())
            else {
                continue;
            };
            let directory = if path.is_dir() {
                path.as_path()
            } else if let Some(parent) = path.parent() {
                parent
            } else {
                continue;
            };
            let Ok(directory) = directory.canonicalize() else {
                continue;
            };
            if !directory.starts_with(root) {
                continue;
            }
            for ancestor in directory
                .ancestors()
                .take_while(|dir| dir.starts_with(root))
            {
                directories.insert((root.clone(), ancestor.to_path_buf()));
            }
        }
        let mut instructions = Vec::new();
        let mut seen = self.seen;
        for (root, directory) in directories {
            let Ok(path) = directory.join("AGENTS.md").canonicalize() else {
                continue;
            };
            if !path.starts_with(&root) || startup.contains(&path) || seen.contains(&path) {
                continue;
            }
            let selector = path.strip_prefix(&root).unwrap_or(&path).to_string_lossy();
            if [selector.as_ref(), path.to_string_lossy().as_ref()]
                .into_iter()
                .any(|selector| {
                    self.policy
                        .check("read", selector, Some(&self.agent_policy))
                        != PermissionAction::Allow
                })
            {
                continue;
            }
            let Some((content, truncated)) = read_instructions(&path, &root, self.max_bytes) else {
                continue;
            };
            let relative = directory.strip_prefix(&root).unwrap_or(&directory);
            let scope = if relative.as_os_str().is_empty() {
                ".".into()
            } else {
                relative.to_string_lossy()
            };
            let mut body = format!(
                "Instructions for directory {scope} (relative to the workspace root). These take precedence over broader project instructions for files in this directory and its descendants.\n\n{content}"
            );
            if truncated {
                body.push_str(&format!(
                    "\n\n[Instruction file truncated at {} bytes.]",
                    self.max_bytes
                ));
            }
            seen.insert(path.clone());
            instructions.push(Instruction { path, body });
        }
        instructions
    }
}

fn read_instructions(path: &Path, root: &Path, max_bytes: u32) -> Option<(String, bool)> {
    if !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    #[cfg(unix)]
    let file = File::from(
        rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .ok()?,
    );
    #[cfg(not(unix))]
    let file = File::open(path).ok()?;
    let opened = file.metadata().ok()?;
    let fresh = path.canonicalize().ok()?;
    if fresh != path || !fresh.starts_with(root) || !opened.is_file() {
        return None;
    }
    let current = std::fs::metadata(&fresh).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (opened.dev(), opened.ino()) != (current.dev(), current.ino()) {
            return None;
        }
    }
    if !current.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(u64::from(max_bytes) + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let truncated = bytes.len() > max_bytes as usize;
    bytes.truncate(max_bytes as usize);
    let content = match String::from_utf8(bytes) {
        Ok(content) => content,
        Err(error) if truncated && error.utf8_error().error_len().is_none() => {
            let valid = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).ok()?
        }
        Err(_) => return None,
    };
    Some((content, truncated))
}

#[cfg(test)]
#[path = "instructions_tests.rs"]
mod tests;

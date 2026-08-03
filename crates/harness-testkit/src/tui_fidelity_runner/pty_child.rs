use std::collections::BTreeSet;
use std::time::Duration;

use portable_pty::Child;

use super::cleanup::ProcessCleanup;
use super::error::RunnerError;
use super::process_tree::{descendants, terminate_group, terminate_pids, wait_for_living};
use crate::tui_fidelity::AdapterKind;

type PtyChild = Box<dyn Child + Send + Sync>;

pub(super) struct PtyChildGuard {
    child: Option<PtyChild>,
    pid: u32,
    observed: BTreeSet<u32>,
    cleanup_timeout: Duration,
}

impl PtyChildGuard {
    pub fn new(child: PtyChild, pid: u32, cleanup_timeout: Duration) -> Self {
        Self {
            child: Some(child),
            pid,
            observed: BTreeSet::new(),
            cleanup_timeout,
        }
    }

    pub const fn pid(&self) -> u32 {
        self.pid
    }

    pub fn parts_mut(
        &mut self,
        adapter: AdapterKind,
    ) -> Result<(&mut PtyChild, &mut BTreeSet<u32>), RunnerError> {
        let child = self.child.as_mut().ok_or_else(|| RunnerError::Process {
            adapter,
            detail: "PTY child was already reaped".to_owned(),
        })?;
        Ok((child, &mut self.observed))
    }

    pub fn cleanup(&mut self) -> ProcessCleanup {
        self.observed.extend(descendants(self.pid));
        let mut forced = false;
        if let Some(child) = self.child.as_mut() {
            if !matches!(child.try_wait(), Ok(Some(_))) {
                forced = true;
                terminate_group(self.pid, self.cleanup_timeout);
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        self.child.take();
        let detected = wait_for_living(&self.observed, self.cleanup_timeout);
        let surviving = if detected.is_empty() {
            Vec::new()
        } else {
            forced = true;
            terminate_pids(&detected);
            wait_for_living(&self.observed, self.cleanup_timeout)
        };
        ProcessCleanup {
            forced_termination: forced,
            detected_child_pids: detected,
            surviving_pids: surviving,
        }
    }
}

impl Drop for PtyChildGuard {
    fn drop(&mut self) {
        if self.child.is_some() {
            let _ = self.cleanup();
        }
    }
}

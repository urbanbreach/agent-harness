use std::collections::BTreeSet;
use std::fs;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanupReport {
    pub forced_termination: bool,
    pub detected_child_pids: Vec<u32>,
    pub surviving_pids: Vec<u32>,
}

impl std::fmt::Display for CleanupReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "forced={}, detected=", self.forced_termination)?;
        write_pid_list(formatter, &self.detected_child_pids)?;
        formatter.write_str(", surviving=")?;
        write_pid_list(formatter, &self.surviving_pids)
    }
}

fn write_pid_list(formatter: &mut std::fmt::Formatter<'_>, pids: &[u32]) -> std::fmt::Result {
    formatter.write_str("[")?;
    for (index, pid) in pids.iter().enumerate() {
        if index > 0 {
            formatter.write_str(", ")?;
        }
        write!(formatter, "{pid}")?;
    }
    formatter.write_str("]")
}

pub(crate) struct ChildCleanup {
    child: Option<Child>,
    pid: u32,
    observed: BTreeSet<u32>,
    timeout: Duration,
}

impl ChildCleanup {
    pub(crate) fn new(child: Child, timeout: Duration) -> Self {
        let pid = child.id();
        Self {
            child: Some(child),
            pid,
            observed: BTreeSet::new(),
            timeout,
        }
    }

    pub(crate) fn child(&mut self) -> Option<&mut Child> {
        self.child.as_mut()
    }

    pub(crate) fn observe(&mut self) {
        self.observed.extend(descendants(self.pid));
    }

    pub(crate) fn finish(mut self) -> CleanupReport {
        self.child.take();
        self.observe();
        let detected = living(&self.observed);
        let surviving = terminate_observed(&detected, self.timeout);
        CleanupReport {
            forced_termination: !detected.is_empty(),
            detected_child_pids: detected,
            surviving_pids: surviving,
        }
    }

    pub(crate) fn terminate_and_reap(&mut self) -> CleanupReport {
        self.observe();
        terminate_group(self.pid, self.timeout);
        if let Some(child) = self.child.take() {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
        }
        let detected = living(&self.observed);
        let surviving = terminate_observed(&detected, self.timeout);
        CleanupReport {
            forced_termination: true,
            detected_child_pids: detected,
            surviving_pids: surviving,
        }
    }
}

impl Drop for ChildCleanup {
    fn drop(&mut self) {
        if self.child.is_some() {
            let _ = self.terminate_and_reap();
        }
    }
}

fn descendants(root: u32) -> BTreeSet<u32> {
    let mut by_parent = std::collections::BTreeMap::<u32, Vec<u32>>::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return BTreeSet::new();
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse().ok())
        else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Some(end) = stat.rfind(')') else {
            continue;
        };
        let Some(parent) = stat[end + 1..]
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        by_parent.entry(parent).or_default().push(pid);
    }
    let mut found = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(parent) = pending.pop() {
        if let Some(children) = by_parent.get(&parent) {
            for child in children {
                if found.insert(*child) {
                    pending.push(*child);
                }
            }
        }
    }
    found
}

fn terminate_observed(pids: &[u32], timeout: Duration) -> Vec<u32> {
    for pid in pids {
        signal(*pid, "-KILL");
    }
    wait_for_living(pids, timeout)
}

fn terminate_group(pid: u32, timeout: Duration) {
    signal(pid, "-TERM");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline && process_exists(pid) {
        thread::sleep(Duration::from_millis(5));
    }
    if process_exists(pid) {
        signal(pid, "-KILL");
    }
}

fn signal(pid: u32, signal: &str) {
    let target = format!("-{pid}");
    let Ok(mut child) = Command::new("kill")
        .args([signal, "--", target.as_str()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let _ = child.wait();
}

fn wait_for_living(pids: &[u32], timeout: Duration) -> Vec<u32> {
    let deadline = Instant::now() + timeout;
    loop {
        let living = living(&pids.iter().copied().collect());
        if living.is_empty() || Instant::now() >= deadline {
            return living;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn living(pids: &BTreeSet<u32>) -> Vec<u32> {
    pids.iter()
        .copied()
        .filter(|pid| process_exists(*pid))
        .collect()
}

fn process_exists(pid: u32) -> bool {
    let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    stat.rfind(')')
        .and_then(|end| stat[end + 1..].split_whitespace().next())
        .is_some_and(|state| state != "Z")
}

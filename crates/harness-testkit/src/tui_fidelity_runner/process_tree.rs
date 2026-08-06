use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) fn descendants(root: u32) -> BTreeSet<u32> {
    let mut by_parent = BTreeMap::<u32, Vec<u32>>::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return BTreeSet::new();
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
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

pub(crate) fn terminate_group(pid: u32, timeout: Duration) {
    signal(&format!("-{pid}"), "-TERM");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline && process_exists(pid) {
        thread::sleep(Duration::from_millis(10));
    }
    if process_exists(pid) {
        signal(&format!("-{pid}"), "-KILL");
    }
}

pub(crate) fn terminate_pids(pids: &[u32]) {
    for pid in pids {
        signal(&pid.to_string(), "-KILL");
    }
}

pub(crate) fn living(pids: &BTreeSet<u32>) -> Vec<u32> {
    pids.iter()
        .copied()
        .filter(|pid| process_exists(*pid))
        .collect()
}

pub(crate) fn wait_for_living(pids: &BTreeSet<u32>, timeout: Duration) -> Vec<u32> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = living(pids);
        if remaining.is_empty() || Instant::now() >= deadline {
            return remaining;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn signal(target: &str, signal: &str) {
    let Ok(mut child) = Command::new("kill")
        .args([signal, "--", target])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        }
    }
}

fn process_exists(pid: u32) -> bool {
    let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    stat.rfind(')')
        .and_then(|end| stat[end + 1..].split_whitespace().next())
        .is_some_and(|state| state != "Z")
}

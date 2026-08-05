use std::collections::BTreeSet;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::tui_fidelity_runner::process_tree::{
    descendants, living, terminate_group, terminate_pids, wait_for_living,
};

use super::{CommandReceipt, CommandStatus, DeadlineError, InterruptFlag, ProcessCleanup};

pub(super) fn run(
    mut command: Command,
    timeout: Duration,
    cleanup_timeout: Duration,
    interrupt: &InterruptFlag,
) -> Result<CommandReceipt, DeadlineError> {
    let started = Instant::now();
    command
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| DeadlineError::Spawn(error.to_string()))?;
    let pid = child.id();
    let stdout = reader(child.stdout.take());
    let stderr = reader(child.stderr.take());
    let mut observed = BTreeSet::new();
    let deadline = started + timeout;
    let (status, exit_code, cleanup) = loop {
        observed.extend(descendants(pid));
        if let Some(exit) = child
            .try_wait()
            .map_err(|error| DeadlineError::Wait(error.to_string()))?
        {
            let survivors = wait_for_living(&observed, cleanup_timeout);
            if survivors.is_empty() {
                break (
                    if exit.success() {
                        CommandStatus::Passed
                    } else {
                        CommandStatus::Failed
                    },
                    exit.code(),
                    ProcessCleanup {
                        forced_termination: false,
                        detected_child_pids: Vec::new(),
                        surviving_pids: Vec::new(),
                    },
                );
            }
            terminate_pids(&survivors);
            break (
                CommandStatus::CleanupFailed,
                exit.code(),
                ProcessCleanup {
                    forced_termination: true,
                    detected_child_pids: survivors,
                    surviving_pids: wait_for_living(&observed, cleanup_timeout),
                },
            );
        }
        let terminal_status = if interrupt.is_interrupted() {
            Some(CommandStatus::Interrupted)
        } else if Instant::now() >= deadline {
            Some(CommandStatus::TimedOut)
        } else {
            None
        };
        if let Some(terminal_status) = terminal_status {
            observed.extend(descendants(pid));
            terminate_group(pid, cleanup_timeout);
            let _ = child.kill();
            let _ = child.wait();
            let detected = living(&observed);
            terminate_pids(&detected);
            break (
                terminal_status,
                None,
                ProcessCleanup {
                    forced_termination: true,
                    detected_child_pids: detected,
                    surviving_pids: wait_for_living(&observed, cleanup_timeout),
                },
            );
        }
        thread::sleep(Duration::from_millis(5));
    };
    let stdout = join_reader(stdout)?;
    let stderr = join_reader(stderr)?;
    Ok(CommandReceipt {
        status,
        duration_millis: started.elapsed().as_millis(),
        exit_code,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        cleanup,
    })
}

fn reader<R: Read + Send + 'static>(
    reader: Option<R>,
) -> thread::JoinHandle<std::io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut reader) = reader {
            reader.read_to_end(&mut bytes)?;
        }
        Ok(bytes)
    })
}

fn join_reader(
    handle: thread::JoinHandle<std::io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, DeadlineError> {
    handle
        .join()
        .map_err(|_| DeadlineError::Wait("output reader panicked".to_owned()))?
        .map_err(|error| DeadlineError::Wait(error.to_string()))
}

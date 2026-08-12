use std::collections::BTreeSet;
use std::io::Write;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::{Duration, Instant};

use super::actions::normal_exit_steps_for_state;
use super::error::RunnerError;
use super::process_io::PtyRead;
use super::process_tree::descendants;
use crate::tui_fidelity::{AdapterKind, Viewport};

type PtyChild = Box<dyn portable_pty::Child + Send + Sync>;

#[derive(Clone, Copy)]
enum FrameReadiness {
    Visible,
    Prompt,
}

fn prompt_is_ready(
    adapter: AdapterKind,
    screen: &str,
    stream: &[u8],
    require_grok_identity: bool,
) -> bool {
    screen.contains('❯')
        && match adapter {
            AdapterKind::Grok => {
                let identity_seen = String::from_utf8_lossy(stream).contains("Grok Build");
                !require_grok_identity
                    || (identity_seen
                        && screen.contains("Grok Build")
                        && !screen.contains("Starting session"))
            }
            AdapterKind::Harness => true,
        }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the polling loop owns one PTY lifecycle boundary"
)]
pub(super) fn wait_until(
    tick: u64,
    tick_duration: Duration,
    start: Instant,
    deadline: Instant,
    adapter: AdapterKind,
    child: &mut PtyChild,
    output: &Receiver<PtyRead>,
    stream: &mut Vec<u8>,
    observed: &mut BTreeSet<u32>,
    pid: u32,
) -> Result<(), RunnerError> {
    let target = start + tick_duration.saturating_mul(u32::try_from(tick).unwrap_or(u32::MAX));
    loop {
        drain(output, stream);
        collect_descendants(pid, observed);
        if adapter == AdapterKind::Grok && String::from_utf8_lossy(stream).contains("Skipped") {
            return Err(RunnerError::SkippedReference);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(RunnerError::PrematureExit {
                    adapter,
                    code: i32::try_from(status.exit_code()).unwrap_or(i32::MAX),
                });
            }
            Ok(None) => {}
            Err(error) => return Err(process_error(adapter, "poll child", error)),
        }
        if Instant::now() >= deadline {
            return Err(RunnerError::Timeout { adapter });
        }
        if Instant::now() >= target {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(1));
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "semantic readiness observes one PTY lifecycle boundary"
)]
pub(super) fn wait_for_visible_stable_frame(
    viewport: Viewport,
    deadline: Instant,
    adapter: AdapterKind,
    child: &mut PtyChild,
    output: &Receiver<PtyRead>,
    stream: &mut Vec<u8>,
    observed: &mut BTreeSet<u32>,
    pid: u32,
) -> Result<(), RunnerError> {
    wait_for_stable_frame(
        viewport,
        deadline,
        adapter,
        child,
        output,
        stream,
        observed,
        pid,
        FrameReadiness::Visible,
        false,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "prompt readiness observes one PTY lifecycle boundary"
)]
pub(super) fn wait_for_prompt_ready(
    viewport: Viewport,
    deadline: Instant,
    adapter: AdapterKind,
    child: &mut PtyChild,
    output: &Receiver<PtyRead>,
    stream: &mut Vec<u8>,
    observed: &mut BTreeSet<u32>,
    pid: u32,
    require_grok_identity: bool,
) -> Result<(), RunnerError> {
    wait_for_stable_frame(
        viewport,
        deadline,
        adapter,
        child,
        output,
        stream,
        observed,
        pid,
        FrameReadiness::Prompt,
        require_grok_identity,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "semantic text readiness observes one PTY lifecycle boundary"
)]
pub(super) fn wait_for_text(
    text: &str,
    viewport: Viewport,
    deadline: Instant,
    adapter: AdapterKind,
    child: &mut PtyChild,
    output: &Receiver<PtyRead>,
    stream: &mut Vec<u8>,
    observed: &mut BTreeSet<u32>,
    pid: u32,
) -> Result<crate::parity::SemanticFrame, RunnerError> {
    loop {
        drain(output, stream);
        collect_descendants(pid, observed);
        if let Some(status) = child
            .try_wait()
            .map_err(|error| process_error(adapter, "poll text target", error))?
        {
            return Err(RunnerError::PrematureExit {
                adapter,
                code: i32::try_from(status.exit_code()).unwrap_or(i32::MAX),
            });
        }
        let frame = semantic_frame(stream, viewport);
        if super::semantic_actions::find_text(&frame, text).is_some() {
            return Ok(frame);
        }
        if Instant::now() >= deadline {
            return Err(RunnerError::SemanticTargetMissing {
                text: text.to_owned(),
            });
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "disclosure readiness observes one PTY lifecycle boundary"
)]
pub(super) fn wait_for_text_pair(
    first: &str,
    second: &str,
    viewport: Viewport,
    deadline: Instant,
    adapter: AdapterKind,
    child: &mut PtyChild,
    output: &Receiver<PtyRead>,
    stream: &mut Vec<u8>,
    observed: &mut BTreeSet<u32>,
    pid: u32,
) -> Result<crate::parity::SemanticFrame, RunnerError> {
    loop {
        drain(output, stream);
        collect_descendants(pid, observed);
        if let Some(status) = child
            .try_wait()
            .map_err(|error| process_error(adapter, "poll disclosure target", error))?
        {
            return Err(RunnerError::PrematureExit {
                adapter,
                code: i32::try_from(status.exit_code()).unwrap_or(i32::MAX),
            });
        }
        let frame = semantic_frame(stream, viewport);
        if super::semantic_actions::find_text(&frame, first).is_some()
            && super::semantic_actions::find_text(&frame, second).is_some()
        {
            return Ok(frame);
        }
        if Instant::now() >= deadline {
            return Err(RunnerError::SemanticTargetMissing {
                text: format!("{first} with disclosure {second}"),
            });
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "semantic text closure observes one PTY lifecycle boundary"
)]
pub(super) fn wait_for_text_absent(
    text: &str,
    viewport: Viewport,
    deadline: Instant,
    adapter: AdapterKind,
    child: &mut PtyChild,
    output: &Receiver<PtyRead>,
    stream: &mut Vec<u8>,
    observed: &mut BTreeSet<u32>,
    pid: u32,
) -> Result<(), RunnerError> {
    loop {
        drain(output, stream);
        collect_descendants(pid, observed);
        if let Some(status) = child
            .try_wait()
            .map_err(|error| process_error(adapter, "poll disclosure closure", error))?
        {
            return Err(RunnerError::PrematureExit {
                adapter,
                code: i32::try_from(status.exit_code()).unwrap_or(i32::MAX),
            });
        }
        let frame = semantic_frame(stream, viewport);
        if super::semantic_actions::find_text(&frame, text).is_none() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(RunnerError::DisclosureTransitionMissing {
                transition: "closed",
            });
        }
        thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn semantic_frame(stream: &[u8], viewport: Viewport) -> crate::parity::SemanticFrame {
    let mut parser = vt100::Parser::new(viewport.rows, viewport.cols, 0);
    parser.process(stream);
    crate::parity::semantic_frame_from_vt100_screen(parser.screen())
}

#[expect(
    clippy::too_many_arguments,
    reason = "frame readiness observes one PTY lifecycle boundary"
)]
fn wait_for_stable_frame(
    viewport: Viewport,
    deadline: Instant,
    adapter: AdapterKind,
    child: &mut PtyChild,
    output: &Receiver<PtyRead>,
    stream: &mut Vec<u8>,
    observed: &mut BTreeSet<u32>,
    pid: u32,
    readiness: FrameReadiness,
    require_grok_identity: bool,
) -> Result<(), RunnerError> {
    let mut previous = None;
    let mut stable_polls = 0_u8;
    loop {
        drain(output, stream);
        collect_descendants(pid, observed);
        if adapter == AdapterKind::Grok && String::from_utf8_lossy(stream).contains("Skipped") {
            return Err(RunnerError::SkippedReference);
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| process_error(adapter, "poll semantic state", error))?
        {
            return Err(RunnerError::PrematureExit {
                adapter,
                code: i32::try_from(status.exit_code()).unwrap_or(i32::MAX),
            });
        }
        let mut parser = vt100::Parser::new(viewport.rows, viewport.cols, 0);
        parser.process(stream);
        let screen = parser.screen().contents();
        let ready = match readiness {
            FrameReadiness::Visible => !screen.trim().is_empty(),
            FrameReadiness::Prompt => {
                prompt_is_ready(adapter, &screen, stream, require_grok_identity)
            }
        };
        if ready {
            if previous.as_deref() == Some(screen.as_str()) {
                stable_polls += 1;
                if stable_polls >= 2 {
                    return Ok(());
                }
            } else {
                stable_polls = 0;
                previous = Some(screen);
            }
        }
        if Instant::now() >= deadline {
            return Err(RunnerError::Timeout { adapter });
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub(super) fn request_normal_exit(
    adapter: AdapterKind,
    writer: &mut dyn Write,
    child: &mut PtyChild,
    deadline: Instant,
    pid: u32,
    observed: &mut BTreeSet<u32>,
    active: bool,
) -> Result<Option<i32>, RunnerError> {
    for step in normal_exit_steps_for_state(adapter, active) {
        writer
            .write_all(step.bytes)
            .and_then(|()| writer.flush())
            .map_err(|error| process_error(adapter, "request normal exit", error))?;
        let step_deadline = std::cmp::min(deadline, Instant::now() + step.dwell);
        loop {
            collect_descendants(pid, observed);
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Ok(Some(i32::try_from(status.exit_code()).unwrap_or(i32::MAX)));
                }
                Ok(None) if Instant::now() < step_deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                Ok(None) => break,
                Err(error) => return Err(process_error(adapter, "poll normal exit", error)),
            }
        }
    }
    Ok(None)
}

pub(super) fn drain(receiver: &Receiver<PtyRead>, stream: &mut Vec<u8>) {
    while let Ok(read) = receiver.try_recv() {
        stream.extend(read.bytes);
    }
}

pub(super) fn collect_descendants(pid: u32, observed: &mut BTreeSet<u32>) {
    observed.extend(descendants(pid));
}

pub(super) fn process_error(
    adapter: AdapterKind,
    operation: &str,
    error: impl std::fmt::Display,
) -> RunnerError {
    RunnerError::Process {
        adapter,
        detail: format!("{operation}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::prompt_is_ready;
    use crate::tui_fidelity::AdapterKind;

    #[test]
    fn grok_welcome_prompt_waits_for_startup_footer() {
        assert!(!prompt_is_ready(
            AdapterKind::Grok,
            "Grok Build Beta  0.2.114\n❯\nStarting session…",
            b"Grok Build",
            true
        ));
        assert!(prompt_is_ready(
            AdapterKind::Grok,
            "Grok Build Beta  0.2.114\n❯\nChangelog",
            b"Grok Build",
            true
        ));
        assert!(prompt_is_ready(
            AdapterKind::Grok,
            "fixture\n❯",
            b"fixture",
            false
        ));
    }
}

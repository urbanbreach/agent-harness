use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::fallback::TerminalContext;

const TMUX_QUERY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum TmuxQueryResult<T> {
    Available(T),
    Unsupported,
    #[default]
    Unavailable,
    Error,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct TmuxStartupFacts {
    extended_keys: TmuxQueryResult<String>,
    allow_passthrough_support: TmuxQueryResult<()>,
    allow_passthrough: TmuxQueryResult<String>,
}

trait TmuxOptionQuery {
    fn show_option(&self, option: &str) -> TmuxQueryResult<String>;
    fn option_support(&self, option: &str) -> TmuxQueryResult<()>;
}

struct LiveTmuxQuery;

impl TmuxOptionQuery for LiveTmuxQuery {
    fn show_option(&self, option: &str) -> TmuxQueryResult<String> {
        let output = run_tmux_bounded(&["show-option", "-gqv", option]);
        match output {
            Ok(output) if output.success => {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if value.is_empty() {
                    TmuxQueryResult::Unavailable
                } else {
                    TmuxQueryResult::Available(value)
                }
            }
            Ok(_) => TmuxQueryResult::Unavailable,
            Err(()) => TmuxQueryResult::Error,
        }
    }

    fn option_support(&self, option: &str) -> TmuxQueryResult<()> {
        let output = run_tmux_bounded(&["show-option", "-gv", option]);
        match output {
            Ok(output) if output.success => TmuxQueryResult::Available(()),
            Ok(output) if stderr_identifies_unknown_option(&output.stderr, option) => {
                TmuxQueryResult::Unsupported
            }
            Ok(_) => TmuxQueryResult::Unavailable,
            Err(()) => TmuxQueryResult::Error,
        }
    }
}

struct TmuxCommandOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_tmux_bounded(args: &[&str]) -> Result<TmuxCommandOutput, ()> {
    let mut child = Command::new("tmux")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| ())?;
    let deadline = Instant::now() + TMUX_QUERY_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(15));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(());
            }
        }
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .stdout
        .take()
        .ok_or(())?
        .read_to_end(&mut stdout)
        .map_err(|_| ())?;
    child
        .stderr
        .take()
        .ok_or(())?
        .read_to_end(&mut stderr)
        .map_err(|_| ())?;
    Ok(TmuxCommandOutput {
        success: status.success(),
        stdout,
        stderr,
    })
}

fn stderr_identifies_unknown_option(stderr: &[u8], option: &str) -> bool {
    let invalid = format!("invalid option: {option}");
    let unknown = format!("unknown option: {option}");
    String::from_utf8_lossy(stderr)
        .lines()
        .any(|line| matches!(line.trim(), value if value == invalid || value == unknown))
}

fn collect_tmux_facts(query: &dyn TmuxOptionQuery) -> TmuxStartupFacts {
    let allow_passthrough_support = query.option_support("allow-passthrough");
    let allow_passthrough = match &allow_passthrough_support {
        TmuxQueryResult::Available(()) => query.show_option("allow-passthrough"),
        TmuxQueryResult::Unsupported => TmuxQueryResult::Unsupported,
        TmuxQueryResult::Unavailable => TmuxQueryResult::Unavailable,
        TmuxQueryResult::Error => TmuxQueryResult::Error,
    };
    TmuxStartupFacts {
        extended_keys: query.show_option("extended-keys"),
        allow_passthrough_support,
        allow_passthrough,
    }
}

fn clipboard_warning_required_from_facts(
    context: TerminalContext,
    is_ssh: bool,
    facts: &TmuxStartupFacts,
) -> bool {
    if !is_ssh || !context.is_tmux_backed() {
        return false;
    }
    let extended_keys_off =
        matches!(&facts.extended_keys, TmuxQueryResult::Available(value) if value == "off");
    let passthrough_exists = !matches!(
        facts.allow_passthrough_support,
        TmuxQueryResult::Unsupported
    );
    let passthrough_off = matches!(
        &facts.allow_passthrough,
        TmuxQueryResult::Available(value) if !matches!(value.as_str(), "on" | "all")
    );
    extended_keys_off || (passthrough_exists && passthrough_off)
}

pub(crate) fn clipboard_warning_required(context: TerminalContext, is_ssh: bool) -> bool {
    if !is_ssh || !context.is_tmux_backed() {
        return false;
    }
    let facts = collect_tmux_facts(&LiveTmuxQuery);
    clipboard_warning_required_from_facts(context, is_ssh, &facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{AltScreenMode, TerminalMultiplexer, TerminalName};

    fn tmux_context() -> TerminalContext {
        TerminalContext {
            brand: TerminalName::Ghostty,
            multiplexer: TerminalMultiplexer::Tmux,
            alt_screen: AltScreenMode::Auto,
            is_tty: true,
            is_byobu: false,
        }
    }

    #[test]
    fn warning_surfaces_over_ssh_when_tmux_extended_keys_are_off() {
        // arrange
        let facts = TmuxStartupFacts {
            extended_keys: TmuxQueryResult::Available("off".to_owned()),
            ..TmuxStartupFacts::default()
        };

        // act
        let required = clipboard_warning_required_from_facts(tmux_context(), true, &facts);

        // assert
        assert!(required);
    }

    #[test]
    fn warning_surfaces_over_ssh_when_supported_tmux_passthrough_is_off() {
        // arrange
        let facts = TmuxStartupFacts {
            allow_passthrough_support: TmuxQueryResult::Available(()),
            allow_passthrough: TmuxQueryResult::Available("off".to_owned()),
            ..TmuxStartupFacts::default()
        };

        // act
        let required = clipboard_warning_required_from_facts(tmux_context(), true, &facts);

        // assert
        assert!(required);
    }

    #[test]
    fn warning_stays_suppressed_outside_ssh() {
        // arrange
        let facts = TmuxStartupFacts {
            extended_keys: TmuxQueryResult::Available("off".to_owned()),
            allow_passthrough_support: TmuxQueryResult::Available(()),
            allow_passthrough: TmuxQueryResult::Available("off".to_owned()),
        };

        // act
        let required = clipboard_warning_required_from_facts(tmux_context(), false, &facts);

        // assert
        assert!(!required);
    }

    #[test]
    fn warning_stays_suppressed_outside_tmux() {
        // arrange
        let facts = TmuxStartupFacts {
            extended_keys: TmuxQueryResult::Available("off".to_owned()),
            ..TmuxStartupFacts::default()
        };
        let context = TerminalContext {
            multiplexer: TerminalMultiplexer::Undetected,
            ..tmux_context()
        };

        // act
        let required = clipboard_warning_required_from_facts(context, true, &facts);

        // assert
        assert!(!required);
    }

    #[test]
    fn warning_stays_suppressed_for_good_or_unavailable_tmux_values() {
        // arrange
        // act
        for facts in [
            TmuxStartupFacts {
                extended_keys: TmuxQueryResult::Available("on".to_owned()),
                allow_passthrough_support: TmuxQueryResult::Available(()),
                allow_passthrough: TmuxQueryResult::Available("all".to_owned()),
            },
            TmuxStartupFacts::default(),
        ] {
            let required = clipboard_warning_required_from_facts(tmux_context(), true, &facts);

            // assert
            assert!(!required, "unexpected warning for {facts:?}");
        }
    }

    #[test]
    fn warning_stays_suppressed_when_passthrough_option_is_unsupported() {
        // arrange
        let facts = TmuxStartupFacts {
            allow_passthrough_support: TmuxQueryResult::Unsupported,
            allow_passthrough: TmuxQueryResult::Available("off".to_owned()),
            ..TmuxStartupFacts::default()
        };

        // act
        let required = clipboard_warning_required_from_facts(tmux_context(), true, &facts);

        // assert
        assert!(!required);
    }
}

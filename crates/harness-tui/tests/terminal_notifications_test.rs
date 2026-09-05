#[path = "../src/terminal_notifications/mod.rs"]
mod terminal_notifications;

use std::io::{self, Write};

use terminal_notifications::{
    FocusState, Multiplexer, NotificationEvent, NotificationKind, NotificationPolicy,
    NotificationProtocol, NotificationWriter, ProtocolSet, WriteOutcome,
};

fn event(kind: NotificationKind, tick: u64, title: &str, body: &str) -> NotificationEvent {
    NotificationEvent {
        kind,
        title: title.to_string(),
        body: body.to_string(),
        created_at_tick: tick,
    }
}

#[test]
fn protocols_emit_expected_sequences_and_strip_controls() {
    assert_eq!(
        NotificationProtocol::Osc9.sequence("title", "body"),
        "\x1b]9;body\x07"
    );
    assert_eq!(
        NotificationProtocol::Osc99.sequence("title", "body"),
        "\x1b]99;i=ID:title;body\x07"
    );
    assert_eq!(
        NotificationProtocol::Osc777.sequence("title", "body"),
        "\x1b]777;notify;title;body\x07"
    );
    assert_eq!(NotificationProtocol::Bell.sequence("title", "body"), "\x07");
    assert_eq!(
        NotificationProtocol::Osc99.sequence("t\0\x1b", "b\x07\n"),
        "\x1b]99;i=ID:t;b\x07"
    );
}

#[test]
fn multiplexer_detection_and_forwarding_are_defined() {
    assert_eq!(
        Multiplexer::Tmux.forwarding_prefix(),
        Some("\x1bPtmux;\x1b")
    );
    assert_eq!(Multiplexer::Zellij.forwarding_prefix(), Some("\x1bP"));
    assert_eq!(Multiplexer::Tmux.forwarding_suffix(), Some("\x1b\\"));
    assert_eq!(Multiplexer::Zellij.forwarding_suffix(), Some("\x1b\\"));
    assert_eq!(Multiplexer::Ssh.forwarding_prefix(), None);
    assert_eq!(Multiplexer::None.forwarding_suffix(), None);
    let detected = Multiplexer::detect_from_env();
    assert!(matches!(
        detected,
        Multiplexer::None
            | Multiplexer::Tmux
            | Multiplexer::Zellij
            | Multiplexer::Ssh
            | Multiplexer::WindowsTerminal
            | Multiplexer::Unknown
    ));
}

#[test]
fn protocol_sets_negotiate_and_support_empty_fallback() {
    assert_eq!(ProtocolSet::unsupported().primary(), None);
    assert!(ProtocolSet::unsupported().protocols.is_empty());
    let set = ProtocolSet {
        protocols: vec![NotificationProtocol::Bell, NotificationProtocol::Osc9],
        multiplexer: Multiplexer::None,
    };
    assert_eq!(set.primary(), Some(NotificationProtocol::Bell));
    assert_eq!(
        set.fallback(),
        &[NotificationProtocol::Bell, NotificationProtocol::Osc9]
    );
}

#[test]
fn policy_respects_focus_action_required_and_unfocused_events() {
    // arrange
    // act
    let mut policy = NotificationPolicy::new(0, 10);
    policy.set_focus(FocusState::Focused);
    // assert
    assert!(!policy.should_notify(&event(NotificationKind::Info, 1, "i", "b")));
    assert!(policy.should_notify(&event(NotificationKind::ActionRequired, 2, "a", "b")));
    policy.set_focus(FocusState::Unfocused);
    assert!(policy.should_notify(&event(NotificationKind::Complete, 3, "c", "b")));
}

#[test]
fn policy_deduplicates_rate_limits_and_resets_window() {
    // arrange
    // act
    let mut policy = NotificationPolicy::new(5, 2);
    // assert
    assert!(policy.should_notify(&event(NotificationKind::Info, 1, "same", "body")));
    assert!(!policy.should_notify(&event(NotificationKind::Info, 3, "same", "body")));
    assert!(policy.should_notify(&event(NotificationKind::Info, 4, "other", "body")));
    assert!(!policy.should_notify(&event(NotificationKind::Info, 5, "third", "body")));
    assert!(policy.should_notify(&event(NotificationKind::Info, 104, "fresh", "body")));
}

#[test]
fn policy_suppression_ends_at_requested_tick() {
    // arrange
    // act
    let mut policy = NotificationPolicy::new(0, 10);
    policy.suppress_for(3, 10);
    // assert
    assert!(!policy.should_notify(&event(NotificationKind::Info, 12, "x", "y")));
    assert!(policy.should_notify(&event(NotificationKind::Info, 13, "x", "y")));
}

struct FailOnce {
    failed: bool,
    output: Vec<u8>,
}
impl Write for FailOnce {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.failed {
            self.failed = true;
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
        }
        self.output.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn writer_writes_primary_falls_back_and_handles_unsupported() {
    // arrange
    // act
    let event = event(NotificationKind::Info, 1, "title", "body");
    let set = ProtocolSet {
        protocols: vec![NotificationProtocol::Osc9, NotificationProtocol::Bell],
        multiplexer: Multiplexer::None,
    };
    let writer = NotificationWriter::new(set);
    let mut output = Vec::new();
    // assert
    assert_eq!(
        writer.write(&event, &mut output),
        WriteOutcome::Written {
            protocol: NotificationProtocol::Osc9,
            bytes: 9
        }
    );
    let mut failing = FailOnce {
        failed: false,
        output: Vec::new(),
    };
    assert_eq!(
        writer.write(&event, &mut failing),
        WriteOutcome::Written {
            protocol: NotificationProtocol::Bell,
            bytes: 1
        }
    );
    assert_eq!(
        NotificationWriter::new(ProtocolSet::unsupported()).write(&event, &mut output),
        WriteOutcome::FallbackExhausted
    );
}

#[test]
fn writer_shutdown_is_best_effort() {
    // arrange
    // act
    let writer = NotificationWriter::new(ProtocolSet::unsupported());
    let mut failing = FailOnce {
        failed: false,
        output: Vec::new(),
    };
    // assert
    assert!(writer.shutdown(&mut failing).is_ok());
}

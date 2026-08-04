use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationProtocol {
    Osc9,
    Osc99,
    Osc777,
    Bell,
}

impl NotificationProtocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Osc9 => "osc9",
            Self::Osc99 => "osc99",
            Self::Osc777 => "osc777",
            Self::Bell => "bell",
        }
    }

    pub fn sequence(self, title: &str, body: &str) -> String {
        let sanitize = |value: &str| {
            value
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
        };
        let title = sanitize(title);
        let body = sanitize(body);
        match self {
            Self::Osc9 => format!("\x1b]9;{body}\x07"),
            Self::Osc99 => format!("\x1b]99;i=ID:{title};{body}\x07"),
            Self::Osc777 => format!("\x1b]777;notify;{title};{body}\x07"),
            Self::Bell => "\x07".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Multiplexer {
    None,
    Tmux,
    Zellij,
    Ssh,
    WindowsTerminal,
    Unknown,
}

impl Multiplexer {
    pub fn detect_from_env() -> Self {
        if std::env::var_os("TMUX").is_some() {
            Self::Tmux
        } else if std::env::var_os("ZELLIJ").is_some() {
            Self::Zellij
        } else if std::env::var_os("SSH_CONNECTION").is_some() {
            Self::Ssh
        } else if std::env::var_os("WT_SESSION").is_some() {
            Self::WindowsTerminal
        } else {
            Self::None
        }
    }

    pub fn forwarding_prefix(self) -> Option<&'static str> {
        match self {
            Self::Tmux => Some("\x1bPtmux;\x1b"),
            Self::Zellij => Some("\x1bP"),
            Self::None | Self::Ssh | Self::WindowsTerminal | Self::Unknown => None,
        }
    }

    pub fn forwarding_suffix(self) -> Option<&'static str> {
        match self {
            Self::Tmux | Self::Zellij => Some("\x1b\\"),
            Self::None | Self::Ssh | Self::WindowsTerminal | Self::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolSet {
    pub protocols: Vec<NotificationProtocol>,
    pub multiplexer: Multiplexer,
}

impl ProtocolSet {
    pub fn negotiate_from_env() -> Self {
        let multiplexer = Multiplexer::detect_from_env();
        let protocols = match multiplexer {
            Multiplexer::WindowsTerminal => vec![
                NotificationProtocol::Osc99,
                NotificationProtocol::Osc777,
                NotificationProtocol::Bell,
            ],
            Multiplexer::Tmux | Multiplexer::Zellij => vec![
                NotificationProtocol::Osc9,
                NotificationProtocol::Osc99,
                NotificationProtocol::Bell,
            ],
            Multiplexer::None | Multiplexer::Ssh | Multiplexer::Unknown => vec![
                NotificationProtocol::Osc99,
                NotificationProtocol::Osc777,
                NotificationProtocol::Osc9,
            ],
        };
        Self {
            protocols,
            multiplexer,
        }
    }

    pub fn primary(&self) -> Option<NotificationProtocol> {
        self.protocols.first().copied()
    }

    pub fn fallback(&self) -> &[NotificationProtocol] {
        &self.protocols
    }

    pub fn unsupported() -> Self {
        Self {
            protocols: Vec::new(),
            multiplexer: Multiplexer::None,
        }
    }
}

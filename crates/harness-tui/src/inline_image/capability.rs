use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsProtocol {
    None,
    Kitty,
    ITerm2,
    Sixel,
}

impl GraphicsProtocol {
    pub const fn supports_truecolor(self) -> bool {
        match self {
            Self::None => false,
            Self::Kitty | Self::ITerm2 | Self::Sixel => true,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Kitty => "kitty",
            Self::ITerm2 => "iterm2",
            Self::Sixel => "sixel",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageCapability {
    pub protocol: GraphicsProtocol,
    pub max_width: u32,
    pub max_height: u32,
    pub negotiated: bool,
}

impl ImageCapability {
    pub const fn unsupported() -> Self {
        Self {
            protocol: GraphicsProtocol::None,
            max_width: 0,
            max_height: 0,
            negotiated: false,
        }
    }

    pub const fn kitty(max_width: u32, max_height: u32) -> Self {
        Self {
            protocol: GraphicsProtocol::Kitty,
            max_width,
            max_height,
            negotiated: true,
        }
    }

    pub const fn iterm2(max_width: u32, max_height: u32) -> Self {
        Self {
            protocol: GraphicsProtocol::ITerm2,
            max_width,
            max_height,
            negotiated: true,
        }
    }

    pub fn negotiate_from_env() -> Self {
        let protocol = match std::env::var("TERM_PROGRAM").as_deref() {
            Ok("WezTerm" | "kitty") => GraphicsProtocol::Kitty,
            Ok("iTerm.app") => GraphicsProtocol::ITerm2,
            _ => GraphicsProtocol::None,
        };
        Self {
            protocol,
            max_width: 1920,
            max_height: 1080,
            negotiated: protocol != GraphicsProtocol::None,
        }
    }

    pub const fn is_available(&self) -> bool {
        !matches!(self.protocol, GraphicsProtocol::None)
    }
}

impl Default for ImageCapability {
    fn default() -> Self {
        Self::unsupported()
    }
}

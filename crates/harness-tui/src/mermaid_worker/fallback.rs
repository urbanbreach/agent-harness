#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MermaidFallback {
    TextBlock(String),
    AsciiArt(String),
    ErrorPlaceholder(String),
}

impl MermaidFallback {
    pub fn render_text(source: &str, width: u16) -> Self {
        let limit = usize::from(width);
        let mut output = String::from("```mermaid\n");
        for line in source.lines() {
            output.push_str(&line.chars().take(limit).collect::<String>());
            output.push('\n');
        }
        output.push_str("```");
        Self::TextBlock(output)
    }

    pub fn render_ascii_placeholder(label: &str, width: u16) -> Self {
        let width = usize::from(width);
        if width < 3 {
            return Self::AsciiArt("+".repeat(width));
        }
        let inner = width - 2;
        let text = label.chars().take(inner).collect::<String>();
        let left = (inner - text.chars().count()) / 2;
        let right = inner - text.chars().count() - left;
        let border = format!("+{}+", "-".repeat(inner));
        Self::AsciiArt(format!(
            "{border}\n|{}{}{}|\n{border}",
            " ".repeat(left),
            text,
            " ".repeat(right)
        ))
    }

    pub fn render_error(message: &str) -> Self {
        Self::ErrorPlaceholder(message.chars().take(200).collect())
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::TextBlock(value) | Self::AsciiArt(value) | Self::ErrorPlaceholder(value) => value,
        }
    }
}

//! Clean-room identity substitution for TUI reference parity.
//!
//! A clean-room reimplementation renders the same functional UI as the
//! pinned reference yet legitimately differs in identity-specific text: the
//! product logo, the product title, the build version, and the operator's
//! provider/account. Truthful parity normalizes exactly that text into
//! generic placeholders and requires every other rendered fragment to match.
//!
//! Truthfulness invariant: only identity spans are rewritten. Product title,
//! provider, and account tokens must appear as whole words, so the literal
//! `Harness` inside `harnessing` is never touched, and versions follow a
//! strict SemVer-ish grammar with dotted-numeric guards so IP addresses and
//! token counts are never touched. Geometry, color, modifiers, and cursor
//! are owned by the cell comparator (see [`super::compare`]) and stay
//! mandatory; this module only normalizes text.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::cells::SemanticFrame;

mod scan;

use scan::{find_exact, find_version, find_word};

/// The bounded set of identity categories a clean-room build may
/// legitimately render differently from the reference.
///
/// Each category maps to one fixed generic placeholder so two
/// otherwise-equivalent renders normalize to byte-identical text. No other
/// category is substitutable: functional content must always match exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityKind {
    /// Product logo glyph or glyph sequence.
    Logo,
    /// Product title word, e.g. `Harness`.
    ProductTitle,
    /// Build version string (SemVer-ish).
    Version,
    /// Provider name.
    Provider,
    /// Operator account identity.
    Account,
}

impl IdentityKind {
    /// The generic placeholder this category normalizes into.
    pub const fn placeholder(self) -> &'static str {
        match self {
            IdentityKind::Logo => "[LOGO]",
            IdentityKind::ProductTitle => "[PRODUCT]",
            IdentityKind::Version => "[VERSION]",
            IdentityKind::Provider => "[PROVIDER]",
            IdentityKind::Account => "[ACCOUNT]",
        }
    }

    /// Tie-break rank when two categories start at the same offset; lower
    /// wins. The logo is most distinctive, the version pattern least.
    const fn tiebreak_rank(self) -> u8 {
        match self {
            IdentityKind::Logo => 0,
            IdentityKind::ProductTitle => 1,
            IdentityKind::Provider => 2,
            IdentityKind::Account => 3,
            IdentityKind::Version => 4,
        }
    }
}

impl fmt::Display for IdentityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.placeholder())
    }
}

/// One identity span rewritten during normalization.
///
/// `original` is the exact source text and `start_byte`/`end_byte` bound its
/// span within the input. Tests and evidence inspect these records to prove
/// truthfulness: exactly the expected categories at the expected positions
/// were rewritten, and nothing else.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityReplacement {
    pub kind: IdentityKind,
    pub original: String,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// Trustworthy identity-substitution registry.
///
/// Holds the concrete identity tokens a build renders (logo, product title,
/// provider, account) plus an opt-in SemVer-ish version rule. Normalization
/// replaces each registered token with its fixed category placeholder and
/// leaves all other text untouched. Every category is opt-in, so an empty
/// registry is the exact identity function.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentitySubstitution {
    #[serde(default)]
    logos: Vec<String>,
    #[serde(default)]
    product_titles: Vec<String>,
    #[serde(default)]
    providers: Vec<String>,
    #[serde(default)]
    accounts: Vec<String>,
    #[serde(default)]
    version_enabled: bool,
}

impl IdentitySubstitution {
    /// An empty registry: normalization is the identity function.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a product logo glyph or glyph sequence (exact match).
    pub fn with_logo(mut self, logo: impl Into<String>) -> Self {
        self.logos.push(logo.into());
        self
    }

    /// Register a product title token (whole-word match).
    pub fn with_product_title(mut self, title: impl Into<String>) -> Self {
        self.product_titles.push(title.into());
        self
    }

    /// Register a provider name (whole-word match).
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.providers.push(provider.into());
        self
    }

    /// Register an account identity (whole-word match).
    pub fn with_account(mut self, account: impl Into<String>) -> Self {
        self.accounts.push(account.into());
        self
    }

    /// Enable the SemVer-ish version rule.
    pub fn with_version(mut self) -> Self {
        self.version_enabled = true;
        self
    }

    /// Normalize identity text in `text`, returning the normalized string.
    pub fn normalize(&self, text: &str) -> String {
        self.normalize_detailed(text).0
    }

    /// Normalize identity text, also returning every span that was rewritten
    /// so callers can audit truthfulness.
    pub fn normalize_detailed(&self, text: &str) -> (String, Vec<IdentityReplacement>) {
        let mut out = String::with_capacity(text.len());
        let mut replacements = Vec::new();
        let mut window = text;
        let mut base = 0usize;
        while let Some(found) = self.find_earliest(window) {
            let (before, tail) = window.split_at(found.start);
            let (matched, rest) = tail.split_at(found.end - found.start);
            out.push_str(before);
            out.push_str(found.kind.placeholder());
            replacements.push(IdentityReplacement {
                kind: found.kind,
                original: matched.to_owned(),
                start_byte: base + found.start,
                end_byte: base + found.end,
            });
            base += found.end;
            window = rest;
        }
        out.push_str(window);
        (out, replacements)
    }

    /// Normalize identity text across every row of a captured frame, one
    /// normalized line per row. Two renders that agree on all functional
    /// content and differ only in identity spans yield identical vectors:
    /// the clean-room text-parity acceptance criterion. Geometry and color
    /// remain enforced by the cell comparator.
    pub fn normalize_frame_lines(&self, frame: &SemanticFrame) -> Vec<String> {
        (0..frame.rows)
            .map(|row| self.normalize(&frame_line(frame, row)))
            .collect()
    }

    /// Find the next identity span at or after the start of `text`, if any.
    fn find_earliest(&self, text: &str) -> Option<Found> {
        let mut best: Option<Found> = None;
        let mut consider = |kind: IdentityKind, span: Option<(usize, usize)>| {
            let Some((start, end)) = span else {
                return;
            };
            let candidate = Found { start, end, kind };
            match best {
                None => best = Some(candidate),
                Some(current) if wins(candidate, current) => best = Some(candidate),
                Some(_) => {}
            }
        };
        for logo in &self.logos {
            consider(IdentityKind::Logo, find_exact(text, 0, logo));
        }
        for title in &self.product_titles {
            consider(IdentityKind::ProductTitle, find_word(text, 0, title));
        }
        for provider in &self.providers {
            consider(IdentityKind::Provider, find_word(text, 0, provider));
        }
        for account in &self.accounts {
            consider(IdentityKind::Account, find_word(text, 0, account));
        }
        if self.version_enabled {
            consider(IdentityKind::Version, find_version(text, 0));
        }
        best
    }
}

/// Reconstruct the visible text of one frame row by joining each
/// non-continuation cell grapheme; blank cells contribute nothing.
pub fn frame_line(frame: &SemanticFrame, row: u16) -> String {
    let mut line = String::new();
    for col in 0..frame.cols {
        if let Some(cell) = frame.cell(row, col) {
            if !cell.continuation {
                line.push_str(&cell.grapheme);
            }
        }
    }
    line
}

/// One candidate identity span discovered in a window.
#[derive(Clone, Copy)]
struct Found {
    start: usize,
    end: usize,
    kind: IdentityKind,
}

/// Tie-break rule: an earlier start wins; equal starts go to the more
/// distinctive category; equal categories prefer the longer span.
fn wins(candidate: Found, current: Found) -> bool {
    if candidate.start != current.start {
        return candidate.start < current.start;
    }
    if candidate.kind.tiebreak_rank() != current.kind.tiebreak_rank() {
        return candidate.kind.tiebreak_rank() < current.kind.tiebreak_rank();
    }
    candidate.end - candidate.start > current.end - current.start
}

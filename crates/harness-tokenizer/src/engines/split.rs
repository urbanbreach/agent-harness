//! Pre-tokenizer scanners for the tiktoken-format encodings.
//!
//! These reproduce the leftmost-first regex alternations of the o200k_base pattern (as shipped by
//! tiktoken-rs 0.7.0) and of the Kimi pattern, including their backtracking, without a regex
//! engine. A regex engine can fail on very long whitespace or letter runs; the scanners are linear
//! and cannot. Both patterns share the numeric, punctuation and whitespace alternatives; they
//! differ in the Han run alternative, the Han exclusion from the case classes, and whether `/`
//! trails a punctuation run.

use unicode_general_category::{get_general_category, GeneralCategory as Gc};
use unicode_script::{Script, UnicodeScript};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Dialect {
    O200k,
    Kimi,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `Lu` and `Lt`.
    Upper,
    /// `Ll`.
    Lower,
    /// `Lm`.
    Modifier,
    /// `Lo`.
    Other,
    /// `M`.
    Mark,
    /// `N`.
    Number,
    /// Everything else.
    Rest,
}

fn kind(ch: char) -> Kind {
    if ch.is_ascii() {
        return match ch {
            'A'..='Z' => Kind::Upper,
            'a'..='z' => Kind::Lower,
            '0'..='9' => Kind::Number,
            _ => Kind::Rest,
        };
    }
    match get_general_category(ch) {
        Gc::UppercaseLetter | Gc::TitlecaseLetter => Kind::Upper,
        Gc::LowercaseLetter => Kind::Lower,
        Gc::ModifierLetter => Kind::Modifier,
        Gc::OtherLetter => Kind::Other,
        Gc::NonspacingMark | Gc::SpacingMark | Gc::EnclosingMark => Kind::Mark,
        Gc::DecimalNumber | Gc::LetterNumber | Gc::OtherNumber => Kind::Number,
        _ => Kind::Rest,
    }
}

const fn is_letter(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Upper | Kind::Lower | Kind::Modifier | Kind::Other
    )
}

/// Scanner over one text; positions are byte offsets on char boundaries.
pub(super) struct Pieces<'a> {
    text: &'a str,
    pos: usize,
    dialect: Dialect,
}

impl<'a> Pieces<'a> {
    pub(super) const fn new(text: &'a str, dialect: Dialect) -> Self {
        Self {
            text,
            pos: 0,
            dialect,
        }
    }

    fn at(&self, pos: usize) -> Option<char> {
        self.text.get(pos..)?.chars().next()
    }

    fn before(&self, pos: usize) -> Option<char> {
        self.text.get(..pos)?.chars().next_back()
    }

    fn han(&self, ch: char) -> bool {
        self.dialect == Dialect::Kimi && ch.script() == Script::Han
    }

    /// `[\p{Lu}\p{Lt}\p{Lm}\p{Lo}\p{M}]`, minus Han for Kimi.
    fn in_upper_class(&self, ch: char) -> bool {
        matches!(
            kind(ch),
            Kind::Upper | Kind::Modifier | Kind::Other | Kind::Mark
        ) && !self.han(ch)
    }

    /// `[\p{Ll}\p{Lm}\p{Lo}\p{M}]`, minus Han for Kimi.
    fn in_lower_class(&self, ch: char) -> bool {
        matches!(
            kind(ch),
            Kind::Lower | Kind::Modifier | Kind::Other | Kind::Mark
        ) && !self.han(ch)
    }

    fn skip_while(&self, mut pos: usize, test: impl Fn(&Self, char) -> bool) -> usize {
        while let Some(ch) = self.at(pos) {
            if !test(self, ch) {
                break;
            }
            pos += ch.len_utf8();
        }
        pos
    }

    /// Optional case-insensitive contraction suffix after a word.
    fn suffix(&self, pos: usize) -> usize {
        if self.at(pos) != Some('\'') {
            return pos;
        }
        let start = pos + 1;
        let Some(rest) = self.text.get(start..) else {
            return pos;
        };
        let mut chars = rest.chars();
        let (Some(first), second) = (chars.next(), chars.next()) else {
            return pos;
        };
        let lower = |c: char| c.to_ascii_lowercase();
        let end = match lower(first) {
            's' | 'ſ' => 1,
            't' | 'm' | 'd' => 1,
            'r' if second.is_some_and(|c| lower(c) == 'e') => 2,
            'v' if second.is_some_and(|c| lower(c) == 'e') => 2,
            'l' if second.is_some_and(|c| lower(c) == 'l') => 2,
            _ => return pos,
        };
        let width: usize = rest.chars().take(end).map(char::len_utf8).sum();
        start + width
    }

    /// `X? U* L+ suffix?`: lowercase-bearing word, optionally led by one non-letter.
    fn word_with_lower(&self, start: usize) -> Option<usize> {
        if let Some(first) = self.at(start)
            && Self::is_prefix(first)
            && let Some(end) = self.word_with_lower_body(start + first.len_utf8())
        {
            return Some(end);
        }
        self.word_with_lower_body(start)
    }

    fn word_with_lower_body(&self, start: usize) -> Option<usize> {
        let upper_end = self.skip_while(start, Self::in_upper_class);
        // Backtrack the greedy upper run until a lowercase-class char can start `L+`.
        let mut anchor = upper_end;
        while !self.at(anchor).is_some_and(|c| self.in_lower_class(c)) {
            if anchor == start {
                return None;
            }
            anchor -= self.before(anchor)?.len_utf8();
        }
        let end = self.skip_while(anchor, Self::in_lower_class);
        Some(self.suffix(end))
    }

    /// `X? U+ L* suffix?`: uppercase-led word, optionally led by one non-letter.
    fn word_with_upper(&self, start: usize) -> Option<usize> {
        if let Some(first) = self.at(start)
            && Self::is_prefix(first)
            && let Some(end) = self.word_with_upper_body(start + first.len_utf8())
        {
            return Some(end);
        }
        self.word_with_upper_body(start)
    }

    fn word_with_upper_body(&self, start: usize) -> Option<usize> {
        let upper_end = self.skip_while(start, Self::in_upper_class);
        if upper_end == start {
            return None;
        }
        let end = self.skip_while(upper_end, Self::in_lower_class);
        Some(self.suffix(end))
    }

    /// `[^\r\n\p{L}\p{N}]`
    fn is_prefix(ch: char) -> bool {
        let kind = kind(ch);
        ch != '\r' && ch != '\n' && !is_letter(kind) && kind != Kind::Number
    }

    /// `\p{N}{1,3}`
    fn digits(&self, start: usize) -> Option<usize> {
        let mut pos = start;
        for _ in 0..3 {
            match self.at(pos) {
                Some(ch) if kind(ch) == Kind::Number => pos += ch.len_utf8(),
                _ => break,
            }
        }
        (pos > start).then_some(pos)
    }

    /// ` ?[^\s\p{L}\p{N}]+[\r\n/]*` (`/` only for o200k).
    fn punctuation(&self, start: usize) -> Option<usize> {
        if self.at(start) == Some(' ')
            && let Some(end) = self.punctuation_body(start + 1)
        {
            return Some(end);
        }
        self.punctuation_body(start)
    }

    fn punctuation_body(&self, start: usize) -> Option<usize> {
        let symbols_end = self.skip_while(start, |_, ch| {
            let kind = kind(ch);
            !ch.is_whitespace() && !is_letter(kind) && kind != Kind::Number
        });
        if symbols_end == start {
            return None;
        }
        let slash = self.dialect == Dialect::O200k;
        Some(self.skip_while(symbols_end, |_, ch| {
            matches!(ch, '\r' | '\n') || (slash && ch == '/')
        }))
    }

    /// `\s*[\r\n]+`, `\s+(?!\S)` and `\s+`, in that priority order.
    fn whitespace(&self, start: usize) -> Option<usize> {
        let run_end = self.skip_while(start, |_, ch| ch.is_whitespace());
        if run_end == start {
            return None;
        }
        // `\s*[\r\n]+`: the greedy run gives back chars until the last newline in it.
        let mut pos = run_end;
        while pos > start {
            let ch = self.before(pos)?;
            pos -= ch.len_utf8();
            if matches!(ch, '\r' | '\n') {
                return Some(pos + 1);
            }
        }
        // `\s+(?!\S)`: leave the last whitespace char for the next piece when a non-space follows.
        if self.at(run_end).is_none() {
            return Some(run_end);
        }
        let last = run_end - self.before(run_end)?.len_utf8();
        if last > start {
            Some(last)
        } else {
            Some(run_end)
        }
    }

    fn piece_end(&self, start: usize, first: char) -> usize {
        if self.dialect == Dialect::Kimi && self.han(first) {
            return self.skip_while(start, |this, ch| this.han(ch));
        }
        self.word_with_lower(start)
            .or_else(|| self.word_with_upper(start))
            .or_else(|| self.digits(start))
            .or_else(|| self.punctuation(start))
            .or_else(|| self.whitespace(start))
            .unwrap_or(start + first.len_utf8())
    }
}

impl<'a> Iterator for Pieces<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let first = self.at(self.pos)?;
        let end = self
            .piece_end(self.pos, first)
            .max(self.pos + first.len_utf8());
        let piece = self.text.get(self.pos..end)?;
        self.pos = end;
        Some(piece)
    }
}

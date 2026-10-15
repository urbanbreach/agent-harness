//! Text to marked stream: normalization, class runs, case marking and boundary markers.
//!
//! Ported from the public ctok project (MIT); see `LICENSES.md`.

use unicode_normalization::UnicodeNormalization;

use super::chars::{
    category, classify, digit_border, escape_marker, is_contraction_suffix, is_digit_run,
    is_funny_space, is_letter, is_lower, is_mark, is_new_cased, is_private_use, is_selector,
    is_stray_mark, is_stripped_control, is_upper, lower_char, lower_into, marks_like_punct, Class,
    BOW, CAPS, EOW, SHIFT,
};
use super::Frame;

/// NFC plus the measured folds: controls and private use are dropped, NUL and funny spaces become
/// U+0020, Thai SARA AM composes, and (v3 only) curly quotes fold to ASCII.
pub(super) fn normalize(text: &str, fold_quotes: bool) -> String {
    let mut cleaned = String::with_capacity(text.len());
    for ch in text.nfc() {
        if is_stripped_control(ch) {
            continue;
        }
        cleaned.push(if ch == '\0' { ' ' } else { ch });
    }
    let composed;
    let cleaned = if cleaned.contains("\u{0E4D}\u{0E32}") {
        composed = cleaned.replace("\u{0E4D}\u{0E32}", "\u{0E33}");
        &composed
    } else {
        &cleaned
    };
    let mut out = String::with_capacity(cleaned.len());
    for ch in cleaned.chars() {
        if is_private_use(ch) {
            continue;
        }
        out.push(match ch {
            '\u{2018}' | '\u{2019}' if fold_quotes => '\'',
            '\u{201C}' | '\u{201D}' if fold_quotes => '"',
            _ if is_funny_space(ch) => ' ',
            _ => ch,
        });
    }
    out
}

struct Run {
    class: Class,
    body: Vec<char>,
}

fn hard_bow(body: &[char]) -> bool {
    body.first()
        .is_some_and(|&c| is_selector(c) || marks_like_punct(c))
}

fn hard_eow(body: &[char]) -> bool {
    body.last()
        .is_some_and(|&c| is_selector(c) || marks_like_punct(c))
}

fn digit_bow(body: &[char]) -> bool {
    is_digit_run(body) && body.first().is_some_and(|&c| digit_border(c))
}

fn digit_eow(body: &[char]) -> bool {
    is_digit_run(body) && body.last().is_some_and(|&c| digit_border(c))
}

fn takes_right_border(run: &Run) -> bool {
    run.class == Class::Punct
        || hard_eow(&run.body)
        || (matches!(run.class, Class::Digit | Class::Hard)
            && is_digit_run(&run.body)
            && digit_eow(&run.body))
}

fn is_lone_apostrophe(run: &Run) -> bool {
    run.body == ['\'']
}

/// A lone apostrophe that opens the word after it.
fn opens_word(runs: &[Run], i: usize) -> bool {
    is_lone_apostrophe(&runs[i])
        && runs
            .get(i + 1)
            .is_some_and(|next| matches!(next.class, Class::Wordy | Class::Stray))
}

/// Whether a lone apostrophe left of wordy run `i` supplies that word's opening boundary.
fn contraction_seam(runs: &[Run], i: usize) -> bool {
    if i == 0 || !is_contraction_suffix(&runs[i].body) {
        return false;
    }
    let prev = &runs[i - 1];
    if prev.class != Class::Punct || !is_lone_apostrophe(prev) {
        return false;
    }
    i < 2 || !takes_right_border(&runs[i - 2])
}

#[derive(PartialEq, Eq)]
enum HardKind {
    Punct,
    Number,
    Letter,
}

fn hard_kind(ch: char) -> HardKind {
    if marks_like_punct(ch) {
        HardKind::Punct
    } else if digit_border(ch) {
        HardKind::Number
    } else {
        HardKind::Letter
    }
}

/// The text split into maximal same-class runs; hard runs are split where the character kind
/// changes.
fn runs(norm: &[char]) -> Vec<Run> {
    let Some((&first, rest)) = norm.split_first() else {
        return Vec::new();
    };
    let mut out: Vec<Run> = Vec::new();
    let mut cur = vec![first];
    let mut cur_class = classify(first);
    if cur_class == Class::Wordy && is_stray_mark(first) {
        cur_class = Class::Stray;
    }
    for &ch in rest {
        let class = classify(ch);
        if cur_class == Class::Stray && class == Class::Wordy && is_stray_mark(ch) {
            cur.push(ch);
        } else if class == cur_class {
            cur.push(ch);
        } else if class == Class::Wordy && is_stray_mark(ch) && cur_class != Class::Wordy {
            out.push(Run {
                class: cur_class,
                body: std::mem::replace(&mut cur, vec![ch]),
            });
            cur_class = Class::Stray;
        } else {
            out.push(Run {
                class: cur_class,
                body: std::mem::replace(&mut cur, vec![ch]),
            });
            cur_class = class;
        }
    }
    out.push(Run {
        class: cur_class,
        body: cur,
    });

    let mut split = Vec::with_capacity(out.len());
    for run in out {
        if run.class != Class::Hard || run.body.len() == 1 {
            split.push(run);
            continue;
        }
        let mut cur = vec![run.body[0]];
        let mut kind = hard_kind(run.body[0]);
        for &ch in &run.body[1..] {
            if is_selector(ch) || hard_kind(ch) == kind {
                cur.push(ch);
            } else {
                split.push(Run {
                    class: Class::Hard,
                    body: std::mem::replace(&mut cur, vec![ch]),
                });
                kind = hard_kind(ch);
            }
        }
        split.push(Run {
            class: Class::Hard,
            body: cur,
        });
    }
    split
}

fn span_is_upper(span: &[char]) -> bool {
    let mut cased = span
        .iter()
        .filter(|&&c| is_upper(c) || is_lower(c))
        .peekable();
    cased.peek().is_some() && cased.all(|&c| is_upper(c))
}

/// Cased span to the marked form the tiler consumes. A case marker fires only on a whole span.
fn mark_case(span: &[char], allcaps_min: Option<usize>, head_mark: bool) -> Vec<char> {
    let literal = || span.to_vec();
    if head_mark || span.contains(&'ẞ') {
        return literal();
    }
    let Some(&first) = span.first() else {
        return literal();
    };
    let lowered = |chars: &[char]| {
        let mut out = String::new();
        for &c in chars {
            lower_into(c, &mut out);
        }
        out
    };
    if span.contains(&'İ') {
        let tail = &span[1..];
        if is_upper(first) && first != 'İ' && !tail.iter().any(|&c| c != 'İ' && is_upper(c)) {
            let mut out = vec![SHIFT];
            out.extend(lower_char(first).chars());
            for &c in tail {
                if c == 'İ' {
                    out.push(c);
                } else {
                    out.extend(lower_char(c).chars());
                }
            }
            return out;
        }
        return literal();
    }
    let unlowerable = span.iter().any(|&c| {
        (is_new_cased(c) || is_letter(category(c)) || is_mark(category(c)))
            && !is_lower(c)
            && (!is_upper(c) || lower_char(c).chars().eq([c]))
    });
    if let Some(min) = allcaps_min
        && span_is_upper(span)
        && span.len() >= min
        && !unlowerable
    {
        let mut out = vec![CAPS];
        out.extend(
            lowered(span)
                .chars()
                .map(|c| if c == 'ς' { 'σ' } else { c }),
        );
        return out;
    }
    if lower_char(first).chars().eq([first]) {
        return literal();
    }
    if is_upper(first) && !span[1..].iter().any(|&c| is_upper(c)) {
        let mut out = vec![SHIFT];
        out.extend(lowered(span).chars());
        return out;
    }
    literal()
}

/// The marked stream over already-normalized text. `norm` must not contain marker glyphs; they
/// are escaped here. For ladder frames the caller has already measured the trailing newline run.
pub(super) fn stream(norm: &str, frame: &Frame, raw_head_space: bool) -> Vec<char> {
    let mut chars: Vec<char> = norm.chars().map(escape_marker).collect();
    if frame.strips_newlines_here() {
        while chars.last() == Some(&'\n') {
            chars.pop();
        }
    }
    if frame.frame_bow
        && raw_head_space
        && chars.first() == Some(&' ')
        && chars.get(1) != Some(&' ')
    {
        chars.remove(0);
    }
    let runs = runs(&chars);
    if runs.is_empty() {
        return if frame.frame_bow {
            vec![BOW]
        } else {
            Vec::new()
        };
    }
    let first = &runs[0];
    let head_quote = opens_word(&runs, 0);
    let has_own_bow = !head_quote
        && (matches!(first.class, Class::Wordy | Class::Punct | Class::Stray)
            || hard_bow(&first.body)
            || (matches!(first.class, Class::Digit | Class::Hard) && digit_bow(&first.body))
            || (first.class == Class::Space && first.body.first() == Some(&' ')));
    let mut out: Vec<char> = Vec::with_capacity(chars.len() + runs.len() * 2);
    if !has_own_bow && frame.frame_bow {
        out.push(if head_quote { ' ' } else { BOW });
    }
    for i in 0..runs.len() {
        write_run(&mut out, &runs, i, frame);
    }
    apply_seam(&out)
}

/// Whether one side of run `i` touches a single space; only a space counts, since the marker
/// stands in for an absorbed space. The message start counts when the frame ends in a word
/// start; the message end never does.
fn borders_space(runs: &[Run], frame: &Frame, i: usize, side: isize) -> bool {
    let Some(j) = i.checked_add_signed(side) else {
        return frame.frame_bow;
    };
    let Some(neighbour) = runs.get(j).filter(|run| run.class == Class::Space) else {
        return false;
    };
    if side < 0 {
        neighbour.body.last() == Some(&' ')
    } else {
        // A right-hand marker is written for the seam space only, never before two spaces.
        neighbour.body.first() == Some(&' ') && neighbour.body.get(1) != Some(&' ')
    }
}

/// Append run `i` with the boundary markers its class takes.
fn write_run(out: &mut Vec<char>, runs: &[Run], i: usize, frame: &Frame) {
    let run = &runs[i];
    let body = &run.body;
    match run.class {
        Class::Wordy => {
            let fused = i > 0 && runs[i - 1].class == Class::Stray;
            let marked = mark_case(body, frame.allcaps_min, fused);
            let lead = marked
                .iter()
                .take_while(|&&c| c == SHIFT || c == CAPS)
                .count();
            out.extend_from_slice(&marked[..lead]);
            if !(fused || contraction_seam(runs, i)) {
                out.push(BOW);
            }
            out.extend_from_slice(&marked[lead..]);
            out.push(EOW);
        }
        Class::Stray => {
            let letter_follows = runs
                .get(i + 1)
                .is_some_and(|next| next.class == Class::Wordy);
            out.push(BOW);
            out.extend_from_slice(body);
            if !letter_follows {
                out.push(EOW);
            }
        }
        _ if run.class == Class::Punct || hard_bow(body) || hard_eow(body) => {
            let punct = run.class == Class::Punct;
            if borders_space(runs, frame, i, -1)
                && !opens_word(runs, i)
                && (punct || hard_bow(body))
            {
                out.push(BOW);
            }
            out.extend_from_slice(body);
            if borders_space(runs, frame, i, 1) && (punct || hard_eow(body)) {
                out.push(EOW);
            }
        }
        Class::Digit | Class::Hard if is_digit_run(body) => {
            if digit_bow(body) && borders_space(runs, frame, i, -1) {
                out.push(BOW);
            }
            out.extend_from_slice(body);
            if digit_eow(body) && borders_space(runs, frame, i, 1) {
                out.push(EOW);
            }
        }
        _ => out.extend_from_slice(body),
    }
}

/// `EOW ' ' [case markers] BOW` becomes `EOW [case markers] BOW`: a single space between two
/// marked spans is the seam and is not written as a character.
fn apply_seam(stream: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(stream.len());
    let mut i = 0;
    while i < stream.len() {
        if stream[i] != '\n' && stream.get(i + 1) == Some(&EOW) && stream.get(i + 2) == Some(&' ') {
            let mut j = i + 3;
            while matches!(stream.get(j), Some(&SHIFT | &CAPS)) {
                j += 1;
            }
            if stream.get(j) == Some(&BOW) {
                out.push(stream[i]);
                out.push(EOW);
                out.extend_from_slice(&stream[i + 3..j]);
                out.push(BOW);
                i = j + 1;
                continue;
            }
        }
        out.push(stream[i]);
        i += 1;
    }
    out
}

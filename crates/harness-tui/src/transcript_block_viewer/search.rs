use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub byte_range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchNavigation {
    pub match_count: usize,
    pub current_match: Option<usize>,
    pub wrapped: bool,
    pub no_result: bool,
}

impl SearchNavigation {
    const fn empty() -> Self {
        Self {
            match_count: 0,
            current_match: None,
            wrapped: false,
            no_result: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchState {
    query: String,
    matches: Vec<SearchMatch>,
    current: Option<usize>,
    wrapped: bool,
}

impl SearchState {
    pub const fn new() -> Self {
        Self {
            query: String::new(),
            matches: Vec::new(),
            current: None,
            wrapped: false,
        }
    }

    pub fn set_query(&mut self, text: &str, query: &str) -> SearchNavigation {
        self.query.clear();
        self.query.push_str(query);
        self.matches = find_matches(text, query);
        self.current = (!self.matches.is_empty()).then_some(0);
        self.wrapped = false;
        self.navigation()
    }

    pub(super) fn set_wrapped_query(
        &mut self,
        text: &str,
        joiners: &[String],
        query: &str,
    ) -> SearchNavigation {
        self.query = query.to_owned();
        self.matches =
            matcher(query).map_or_else(Vec::new, |regex| wrapped_matches(text, joiners, &regex));
        self.current = (!self.matches.is_empty()).then_some(0);
        self.wrapped = false;
        self.navigation()
    }

    pub fn navigate(&mut self, direction: SearchDirection) -> SearchNavigation {
        self.wrapped = false;
        let Some(current) = self.current else {
            return SearchNavigation::empty();
        };
        let count = self.matches.len();
        if count == 0 {
            return SearchNavigation::empty();
        }
        match direction {
            SearchDirection::Forward => {
                if current + 1 == count {
                    self.current = Some(0);
                    self.wrapped = true;
                } else {
                    self.current = Some(current + 1);
                }
            }
            SearchDirection::Backward => {
                if current == 0 {
                    self.current = Some(count - 1);
                    self.wrapped = true;
                } else {
                    self.current = Some(current - 1);
                }
            }
        }
        self.navigation()
    }

    pub(super) fn select(&mut self, index: usize, wrapped: bool) {
        self.current = self.matches.get(index).map(|_| index);
        self.wrapped = wrapped;
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn matches(&self) -> &[SearchMatch] {
        &self.matches
    }

    pub const fn current_match_index(&self) -> Option<usize> {
        self.current
    }

    pub fn current_match(&self) -> Option<&SearchMatch> {
        self.current.and_then(|index| self.matches.get(index))
    }

    pub const fn no_result(&self) -> bool {
        !self.query.is_empty() && self.matches.is_empty()
    }

    pub(super) fn navigation(&self) -> SearchNavigation {
        if self.matches.is_empty() {
            SearchNavigation::empty()
        } else {
            SearchNavigation {
                match_count: self.matches.len(),
                current_match: self.current,
                wrapped: self.wrapped,
                no_result: false,
            }
        }
    }
}

fn matcher(query: &str) -> Option<regex::Regex> {
    if query.is_empty() {
        return None;
    }
    regex::RegexBuilder::new(query)
        .case_insensitive(!query.chars().any(char::is_uppercase))
        .build()
        .ok()
}

fn find_matches(text: &str, query: &str) -> Vec<SearchMatch> {
    matcher(query).map_or_else(Vec::new, |regex| regex_matches(text, &regex))
}

fn regex_matches(text: &str, matcher: &regex::Regex) -> Vec<SearchMatch> {
    let boundaries = grapheme_boundaries(text);
    matcher
        .find_iter(text)
        .filter_map(|found| {
            let start = found.start();
            let end = found.end();
            (start < end
                && boundaries.binary_search(&start).is_ok()
                && boundaries.binary_search(&end).is_ok())
            .then_some(SearchMatch {
                byte_range: start..end,
            })
        })
        .collect()
}

/// Match each original line, then translate source bytes back to painted rows.
/// Soft-wrap joiners can contain omitted spaces or be empty for a split long word.
fn wrapped_matches(text: &str, joiners: &[String], matcher: &regex::Regex) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    let mut source = String::new();
    let mut rows = Vec::new();
    let mut display_offset = 0;
    for (index, line) in text.split('\n').enumerate() {
        rows.push((source.len(), display_offset, line.len()));
        source.push_str(line);
        display_offset += line.len() + 1;
        let joiner = joiners.get(index).map_or("\n", String::as_str);
        if joiner == "\n" {
            append_wrapped_matches(&mut matches, &source, &rows, matcher);
            source.clear();
            rows.clear();
        } else {
            source.push_str(joiner);
        }
    }
    if !rows.is_empty() {
        append_wrapped_matches(&mut matches, &source, &rows, matcher);
    }
    matches
}

fn append_wrapped_matches(
    matches: &mut Vec<SearchMatch>,
    source: &str,
    rows: &[(usize, usize, usize)],
    matcher: &regex::Regex,
) {
    let display_byte = |byte| {
        let index = rows
            .partition_point(|(source, _, _)| *source <= byte)
            .saturating_sub(1);
        let (source_start, display_start, length) = rows[index];
        display_start + (byte - source_start).min(length)
    };
    matches.extend(
        regex_matches(source, matcher)
            .into_iter()
            .filter_map(|found| {
                let start = display_byte(found.byte_range.start);
                let end = display_byte(found.byte_range.end - 1) + 1;
                (start < end).then_some(SearchMatch {
                    byte_range: start..end,
                })
            }),
    );
}

fn grapheme_boundaries(text: &str) -> Vec<usize> {
    let mut boundaries = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        boundaries.extend(line.grapheme_indices(true).map(|(byte, _)| offset + byte));
        offset += line.len();
        boundaries.push(offset);
        offset += 1;
    }
    boundaries
}

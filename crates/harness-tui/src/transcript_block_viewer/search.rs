use std::ops::Range;

use crate::transcript_selection::{CellPoint, WrappedText};

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

    fn navigation(&self) -> SearchNavigation {
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

fn find_matches(text: &str, query: &str) -> Vec<SearchMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let boundaries = grapheme_boundaries(text);
    text.match_indices(query)
        .filter_map(|(start, _)| {
            let end = start + query.len();
            (boundaries.binary_search(&start).is_ok() && boundaries.binary_search(&end).is_ok())
                .then_some(SearchMatch {
                    byte_range: start..end,
                })
        })
        .collect()
}

fn grapheme_boundaries(text: &str) -> Vec<usize> {
    let lines = text.split('\n').collect::<Vec<_>>();
    let mut boundaries = vec![0];
    let mut offset = 0;
    for (line_index, line) in lines.iter().enumerate() {
        let width = line.len().max(1);
        if let Ok(wrapped) = WrappedText::new(line, width) {
            for cell in 0..=line.len() {
                if let Some(grapheme) = wrapped.grapheme_at(CellPoint::new(0, cell)) {
                    boundaries.push(offset + grapheme.range.byte_range.start);
                    boundaries.push(offset + grapheme.range.byte_range.end);
                }
            }
        }
        offset += line.len();
        boundaries.push(offset);
        if line_index + 1 < lines.len() {
            offset += 1;
            boundaries.push(offset);
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
}

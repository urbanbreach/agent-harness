//! Prompt-cache lookback: how far back a provider cache breakpoint can still reach.
use crate::item::{Item, ItemPart};

/// Positions every request reserves for system and tool framing plus the moving breakpoint.
const RESERVED_POSITIONS: u32 = 6;

/// Walks the projected content blocks backward and returns the first entry index a
/// reducer may touch without invalidating the cached prefix.
///
/// `None` lookback means the provider has no bounded window, so every entry from `boundary`
/// is reachable. Otherwise the walk counts block positions (consecutive tool calls form one
/// position, consecutive results form one position, hoisted error-result images count) and,
/// once the count reaches the lookback, returns the last assistant entry that still fit, so
/// the issuing turn must fit and not only its result. An exhausted walk returns `boundary`.
pub fn lookback_floor(items: &[Item<'_>], boundary: usize, lookback: Option<u32>) -> usize {
    let Some(limit) = lookback else {
        return boundary;
    };
    let mut walk = Walk {
        positions: RESERVED_POSITIONS,
        limit,
        ..Walk::default()
    };
    let mut floor = items.len();
    for index in (boundary..items.len()).rev() {
        let item = &items[index];
        if item.is_tool_result() {
            walk.in_use_run = false;
            walk.assistant_before = false;
            walk.tool_result(item);
        } else if item.is_assistant() {
            walk.assistant(item);
        } else {
            walk.in_result_run = false;
            walk.in_use_run = false;
            walk.assistant_before = false;
            walk.user(item);
        }
        if walk.reached() {
            return floor;
        }
        if item.is_assistant() && item.kind == crate::item::ItemKind::Message {
            floor = index;
        }
    }
    boundary
}

#[derive(Default)]
struct Walk {
    positions: u32,
    limit: u32,
    in_result_run: bool,
    result_run_has_image: bool,
    in_use_run: bool,
    /// The previously processed (newer) message was a nonempty assistant message.
    assistant_before: bool,
}

impl Walk {
    fn reached(&self) -> bool {
        self.positions >= self.limit
    }

    fn add(&mut self, count: u32) {
        self.positions = self.positions.saturating_add(count);
    }

    fn tool_result(&mut self, item: &Item<'_>) {
        if !self.in_result_run {
            self.add(1);
            self.in_result_run = true;
            self.result_run_has_image = false;
        }
        if item.tool.is_error && item.image_count > 0 {
            self.add(item.image_count);
            if !self.result_run_has_image {
                self.add(1);
                self.result_run_has_image = true;
            }
        }
    }

    fn user(&mut self, item: &Item<'_>) {
        self.add(u32::from(!item.text().trim().is_empty()));
        self.add(item.image_count);
    }

    fn assistant(&mut self, item: &Item<'_>) {
        self.in_result_run = false;
        let blocks = blocks(item);
        let nonempty = blocks.iter().any(|block| *block != Block::Blank);
        if nonempty && self.assistant_before {
            self.add(1);
        }
        self.in_use_run = false;
        for block in blocks.iter().rev() {
            match block {
                Block::Blank => {}
                Block::Call => {
                    if !self.in_use_run {
                        self.add(1);
                    }
                    self.in_use_run = true;
                }
                Block::Other => {
                    self.add(1);
                    self.in_use_run = false;
                }
            }
        }
        if nonempty {
            self.assistant_before = true;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    /// Whitespace-only text; ignored.
    Blank,
    Call,
    Other,
}

/// The ordered blocks of an assistant entry. A flat message is reasoning, then text, then calls.
fn blocks(item: &Item<'_>) -> Vec<Block> {
    if !item.parts.is_empty() {
        return item
            .parts
            .iter()
            .map(|part| match part {
                ItemPart::Text(text) if text.trim().is_empty() => Block::Blank,
                ItemPart::Call(_) => Block::Call,
                _ => Block::Other,
            })
            .collect();
    }
    let mut blocks = vec![Block::Other; item.reasoning.len()];
    if !item.text().trim().is_empty() {
        blocks.push(Block::Other);
    }
    if item
        .message
        .assistant_tool_calls
        .as_ref()
        .is_some_and(|calls| !calls.is_empty())
    {
        blocks.push(Block::Call);
    }
    blocks
}

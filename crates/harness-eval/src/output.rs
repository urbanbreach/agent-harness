use crate::Result;
use serde_json::{json, Value};
use std::{fs::File, io::Write, path::PathBuf};

const TAIL_BYTES: usize = 50 * 1024;

/// Keep a bounded preview and spill the original bytes before losing any text.
pub(crate) struct Output {
    path: PathBuf,
    file: Option<File>,
    before_spill: String,
    head: String,
    tail: String,
    head_limit: usize,
    columns: usize,
    line_bytes: usize,
    capped: bool,
    total_bytes: usize,
    newlines: usize,
    dropped_bytes: usize,
    capped_lines: usize,
}

impl Output {
    pub fn new(path: PathBuf, head_limit: usize, columns: usize) -> Self {
        Self {
            path,
            file: None,
            before_spill: String::new(),
            head: String::new(),
            tail: String::new(),
            head_limit,
            columns,
            line_bytes: 0,
            capped: false,
            total_bytes: 0,
            newlines: 0,
            dropped_bytes: 0,
            capped_lines: 0,
        }
    }

    pub fn push(&mut self, text: &str, clamp: bool) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.total_bytes += text.len();
        self.newlines += text.bytes().filter(|byte| *byte == b'\n').count();
        let mut retained = String::new();
        for line in text.split_inclusive('\n') {
            let segment = line.strip_suffix('\n').unwrap_or(line);
            if clamp && self.columns > 0 {
                let end = if self.capped {
                    0
                } else {
                    boundary(segment, self.columns.saturating_sub(self.line_bytes))
                };
                retained.push_str(&segment[..end]);
                self.line_bytes += end;
                if end < segment.len() {
                    self.dropped_bytes += segment.len() - end;
                    self.mark_capped(&mut retained);
                }
            } else {
                retained.push_str(segment);
                self.line_bytes += segment.len();
            }
            if line.ends_with('\n') {
                retained.push('\n');
                self.line_bytes = 0;
                self.capped = false;
            }
        }
        if self.file.is_none()
            && (self.total_bytes > TAIL_BYTES || self.dropped_bytes > 0 || self.newlines >= 2000)
        {
            let mut file = File::create(&self.path)?;
            file.write_all(self.before_spill.as_bytes())?;
            self.before_spill.clear();
            self.file = Some(file);
        }
        if let Some(file) = &mut self.file {
            file.write_all(text.as_bytes())?;
        } else {
            self.before_spill.push_str(text);
        }
        let end = boundary(&retained, self.head_limit.saturating_sub(self.head.len()));
        self.head.push_str(&retained[..end]);
        self.tail.push_str(&retained[end..]);
        if self.tail.len() > TAIL_BYTES {
            let mut start = self.tail.len() - TAIL_BYTES;
            while !self.tail.is_char_boundary(start) {
                start += 1;
            }
            self.tail.drain(..start);
        }
        Ok(())
    }

    pub fn preview(&self) -> String {
        let body = format!("{}{}", self.head, self.tail);
        body.lines()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn release(&mut self) {
        self.file = None;
        self.before_spill = String::new();
        self.head = String::new();
        self.tail = String::new();
    }

    fn mark_capped(&mut self, retained: &mut String) {
        if !self.capped {
            retained.push('…');
            self.capped_lines += 1;
        }
        self.capped = true;
    }

    pub fn snapshot(&self) -> (String, Option<Value>) {
        let mut tail = self.tail.as_str();
        if let Some((offset, _)) = tail.rmatch_indices('\n').nth(1999) {
            tail = &tail[offset + 1..];
        }
        let kept = self.head.len() + tail.len();
        let missing = self.total_bytes.saturating_sub(self.dropped_bytes + kept);
        let missing_lines = (self.newlines + usize::from(self.total_bytes > 0))
            .saturating_sub(lines(&self.head) + lines(tail));
        let output = if missing > 0 && !self.head.is_empty() {
            let marker = if missing_lines > 1 {
                format!("[…{missing_lines}ln elided…]")
            } else {
                format!("[…{missing}B elided…]")
            };
            format!(
                "{}{}{marker}{}{}",
                self.head,
                if self.head.ends_with('\n') { "" } else { "\n" },
                if tail.is_empty() || tail.starts_with('\n') {
                    ""
                } else {
                    "\n"
                },
                tail
            )
        } else {
            format!("{}{tail}", self.head)
        };
        let meta = if self.dropped_bytes > 0 || missing > 0 {
            let middle = missing > 0 && !self.head.is_empty();
            let cause = if middle {
                "middle"
            } else if self.dropped_bytes > 0
                && self.dropped_bytes >= self.total_bytes.saturating_sub(output.len())
            {
                "columns"
            } else if self.total_bytes.saturating_sub(self.dropped_bytes) > TAIL_BYTES {
                "bytes"
            } else {
                "lines"
            };
            let total_lines = self.newlines + usize::from(self.total_bytes > 0);
            let shown_lines = lines(&output);
            let mut meta = json!({"totalLines":self.newlines + usize::from(self.total_bytes > 0),
                "totalBytes":self.total_bytes,"outputLines":shown_lines,"outputBytes":output.len(),
                "direction":if middle {"middle"} else {"tail"},"truncatedBy":cause});
            if middle {
                meta["elidedBytes"] = missing.into();
                meta["elidedLines"] = missing_lines.into();
                let head_lines = shown_lines.saturating_sub(1).div_ceil(2);
                let tail_lines = shown_lines.saturating_sub(1 + head_lines);
                if head_lines > 0 {
                    meta["headRange"] = json!({"start":1,"end":head_lines});
                }
                if tail_lines > 0 {
                    meta["tailRange"] =
                        json!({"start":total_lines.saturating_sub(tail_lines)+1,"end":total_lines});
                }
            } else {
                meta["shownRange"] =
                    json!({"start":total_lines.saturating_sub(shown_lines)+1,"end":total_lines});
            }
            if cause == "columns" {
                meta["maxColumns"] = self.columns.into();
                meta["columnTruncatedLines"] = self.capped_lines.into();
            }
            if cause == "bytes" {
                meta["maxBytes"] = TAIL_BYTES.into();
            }
            if self.file.is_some() {
                meta["artifactId"] = json!(self.path);
            }
            Some(meta)
        } else if self.file.is_some() {
            Some(json!({"artifactId":self.path}))
        } else {
            None
        };
        (output, meta)
    }
}

fn boundary(text: &str, limit: usize) -> usize {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}
fn lines(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.bytes().filter(|byte| *byte == b'\n').count() + 1
    }
}

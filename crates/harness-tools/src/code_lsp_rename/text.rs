use serde_json::Value;

use harness_core::tool::ToolError;

pub(super) struct ParsedTextEdit {
    range: TextByteRange,
    new_text: String,
    annotation_id: Option<String>,
}

impl ParsedTextEdit {
    fn from_lsp_edit(source: &SourceText<'_>, edit: &Value) -> Result<Self, ToolError> {
        let range = edit.get("range").ok_or_else(|| {
            ToolError::Execution("lsp.rename returned a text edit without a range".to_string())
        })?;
        let range = TextByteRange::from_lsp_range(source, range, RangeErrorMessages::text_edit())?;
        let new_text = edit
            .get("newText")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::Execution("lsp.rename returned a text edit without newText".to_string())
            })?
            .to_string();
        let annotation_id = edit
            .get("annotationId")
            .and_then(Value::as_str)
            .map(str::to_string);

        Ok(Self {
            range,
            new_text,
            annotation_id,
        })
    }
}

struct TextPosition {
    line: u64,
    character: u64,
}

impl ParsedTextEdit {
    pub(super) fn annotation_id(&self) -> Option<&str> {
        self.annotation_id.as_deref()
    }
}

impl TextPosition {
    fn from_range_endpoint(
        range: &Value,
        endpoint: &str,
        error_messages: RangePositionErrorMessages,
    ) -> Result<Self, ToolError> {
        let position = range.get(endpoint);
        let line = position
            .and_then(|position| position.get("line"))
            .and_then(Value::as_u64)
            .ok_or_else(|| ToolError::Execution(error_messages.missing_line.to_string()))?;
        let character = position
            .and_then(|position| position.get("character"))
            .and_then(Value::as_u64)
            .ok_or_else(|| ToolError::Execution(error_messages.missing_character.to_string()))?;
        Ok(Self { line, character })
    }

    fn line_index(&self) -> Result<usize, ToolError> {
        usize::try_from(self.line)
            .map_err(|_| ToolError::Execution("line index overflow in workspace edit".to_string()))
    }

    fn character_offset(&self) -> Result<usize, ToolError> {
        usize::try_from(self.character).map_err(|_| {
            ToolError::Execution("character index overflow in workspace edit".to_string())
        })
    }

    fn to_byte_offset(&self, source: &SourceText<'_>) -> Result<usize, ToolError> {
        let line = self.line_index()?;
        let character = self.character_offset()?;
        source.line(line)?.utf16_position_to_byte_offset(character)
    }
}

struct TextByteRange {
    start: usize,
    end: usize,
}

impl TextByteRange {
    fn from_lsp_range(
        source: &SourceText<'_>,
        range: &Value,
        error_messages: RangeErrorMessages,
    ) -> Result<Self, ToolError> {
        let start = TextPosition::from_range_endpoint(range, "start", error_messages.start)?;
        let end = TextPosition::from_range_endpoint(range, "end", error_messages.end)?;
        let start = start.to_byte_offset(source)?;
        let end = end.to_byte_offset(source)?;
        Ok(Self { start, end })
    }

    fn validate_after(&self, previous_end: usize) -> Result<(), ToolError> {
        if self.start > self.end {
            return Err(ToolError::Execution(
                "lsp.rename returned a text edit with an inverted range".to_string(),
            ));
        }
        if self.start < previous_end {
            return Err(ToolError::Execution(
                "lsp.rename returned overlapping text edits".to_string(),
            ));
        }
        Ok(())
    }

    fn replace_in(&self, text: &mut String, replacement: &str) {
        text.replace_range(self.start..self.end, replacement);
    }

    fn slice_from<'a>(&self, text: &'a str) -> &'a str {
        &text[self.start..self.end]
    }
}

pub(super) struct SourceText<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceText<'a> {
    pub(super) fn new(text: &'a str) -> Self {
        Self {
            text,
            line_starts: Self::line_starts(text),
        }
    }

    fn line_starts(text: &str) -> Vec<usize> {
        let mut starts = vec![0usize];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(index + 1);
            }
        }
        starts
    }

    fn line(&self, line: usize) -> Result<SourceLine<'a>, ToolError> {
        let Some(&start) = self.line_starts.get(line) else {
            return Err(ToolError::Execution(format!(
                "workspace edit referenced missing line index {line}"
            )));
        };
        Ok(SourceLine::from_bounds(
            self.text,
            start,
            self.line_end(line),
        ))
    }

    fn line_end(&self, line: usize) -> usize {
        self.line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.text.len())
    }

    pub(super) fn apply_text_edits(&self, edits: &[ParsedTextEdit]) -> Result<String, ToolError> {
        let ordered = Self::ordered_non_overlapping_text_edits(edits)?;

        let mut updated = self.text.to_string();
        for edit in ordered.into_iter().rev() {
            edit.range.replace_in(&mut updated, &edit.new_text);
        }
        Ok(updated)
    }

    fn ordered_non_overlapping_text_edits(
        edits: &[ParsedTextEdit],
    ) -> Result<Vec<&ParsedTextEdit>, ToolError> {
        let mut ordered = edits.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|edit| (edit.range.start, edit.range.end));

        let mut previous_end = 0usize;
        for edit in &ordered {
            edit.range.validate_after(previous_end)?;
            previous_end = edit.range.end;
        }

        Ok(ordered)
    }

    pub(super) fn parse_lsp_text_edits(
        &self,
        edits: &[Value],
    ) -> Result<Vec<ParsedTextEdit>, ToolError> {
        edits
            .iter()
            .map(|edit| ParsedTextEdit::from_lsp_edit(self, edit))
            .collect()
    }

    pub(super) fn text_for_lsp_range(
        &self,
        range: &Value,
        errors: RangeErrorMessages,
    ) -> Result<&'a str, ToolError> {
        let range = TextByteRange::from_lsp_range(self, range, errors)?;
        Ok(range.slice_from(self.text))
    }
}

struct SourceLine<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

impl<'a> SourceLine<'a> {
    fn from_bounds(source: &'a str, start: usize, mut end: usize) -> Self {
        if end > start && source.as_bytes()[end - 1] == b'\n' {
            end -= 1;
            if end > start && source.as_bytes()[end - 1] == b'\r' {
                end -= 1;
            }
        }
        SourceLine {
            text: &source[start..end],
            start,
            end,
        }
    }

    fn utf16_position_to_byte_offset(&self, character: usize) -> Result<usize, ToolError> {
        if character == 0 {
            return Ok(self.start);
        }

        let mut utf16_offset = 0usize;
        for (byte_offset, ch) in self.text.char_indices() {
            if utf16_offset == character {
                return Ok(self.start + byte_offset);
            }
            utf16_offset += ch.len_utf16();
            if utf16_offset == character {
                return Ok(self.start + byte_offset + ch.len_utf8());
            }
            if utf16_offset > character {
                return Err(ToolError::Execution(
                    "workspace edit referenced a non-boundary UTF-16 character offset".to_string(),
                ));
            }
        }
        Ok(self.end)
    }
}

struct RangePositionErrorMessages {
    missing_line: &'static str,
    missing_character: &'static str,
}

pub(super) struct RangeErrorMessages {
    start: RangePositionErrorMessages,
    end: RangePositionErrorMessages,
}

impl RangeErrorMessages {
    pub(super) fn text_edit() -> Self {
        Self {
            start: RangePositionErrorMessages {
                missing_line: "lsp.rename returned a text edit with an invalid start line",
                missing_character:
                    "lsp.rename returned a text edit with an invalid start character",
            },
            end: RangePositionErrorMessages {
                missing_line: "lsp.rename returned a text edit with an invalid end line",
                missing_character: "lsp.rename returned a text edit with an invalid end character",
            },
        }
    }

    pub(super) fn prepare_result() -> Self {
        Self {
            start: RangePositionErrorMessages {
                missing_line: "rename prepare result is missing start.line",
                missing_character: "rename prepare result is missing start.character",
            },
            end: RangePositionErrorMessages {
                missing_line: "rename prepare result is missing end.line",
                missing_character: "rename prepare result is missing end.character",
            },
        }
    }
}

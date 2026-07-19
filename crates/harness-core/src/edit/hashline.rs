// allow: SIZE_OK — hashline editing (anchor validation + atomic apply)
use crate::UnwrapOrAbort;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const HASH_HEX_LEN: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineAnchor {
    pub line: u32,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HashlinePatch {
    pub edit_id: String,
    pub path: String,
    pub ops: Vec<HashlineOp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HashlineWorkspaceOp {
    Patch {
        patch: HashlinePatch,
    },
    RewriteFile {
        edit_id: String,
        path: String,
        content: String,
    },
    DeleteFile {
        edit_id: String,
        path: String,
    },
    MoveFile {
        edit_id: String,
        from_path: String,
        to_path: String,
    },
}

impl HashlineWorkspaceOp {
    pub fn edit_id(&self) -> &str {
        match self {
            Self::Patch { patch } => &patch.edit_id,
            Self::RewriteFile { edit_id, .. }
            | Self::DeleteFile { edit_id, .. }
            | Self::MoveFile { edit_id, .. } => edit_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashlineOp {
    Rewrite {
        lines: Vec<String>,
    },
    InsertBefore {
        anchor: LineAnchor,
        lines: Vec<String>,
    },
    InsertAfter {
        anchor: LineAnchor,
        lines: Vec<String>,
    },
    Replace {
        expected: Vec<LineAnchor>,
        lines: Vec<String>,
    },
    Delete {
        expected: Vec<LineAnchor>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashlineApplyResult {
    pub content: String,
    pub changed_ranges: Vec<ChangedLineRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedLineRange {
    pub start_line: u32,
    pub removed_lines: u32,
    pub added_lines: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(tag = "code", content = "detail")]
pub enum HashlineError {
    #[serde(rename = "ANCHOR_MISMATCH")]
    #[error("anchor mismatch at op {op_index}, line {line}: expected {expected_hash}, got {actual_hash}")]
    AnchorMismatch {
        op_index: usize,
        line: u32,
        expected_hash: String,
        actual_hash: String,
    },
    #[serde(rename = "OUT_OF_RANGE")]
    #[error("line {line} is out of range at op {op_index}; max line is {max_line}")]
    OutOfRange {
        op_index: usize,
        line: u32,
        max_line: u32,
    },
    #[serde(rename = "OVERLAP")]
    #[error("op {first_op_index} overlaps/conflicts with op {second_op_index}: {reason}")]
    Overlap {
        first_op_index: usize,
        second_op_index: usize,
        reason: String,
    },
    #[serde(rename = "EMPTY_PATCH")]
    #[error("edit patch {edit_id} has no operations")]
    EmptyPatch { edit_id: String },
}

impl HashlineError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::AnchorMismatch { .. } => "ANCHOR_MISMATCH",
            Self::OutOfRange { .. } => "OUT_OF_RANGE",
            Self::Overlap { .. } => "OVERLAP",
            Self::EmptyPatch { .. } => "EMPTY_PATCH",
        }
    }
}

#[derive(Debug, Clone)]
enum PlannedOp {
    Rewrite {
        op_index: usize,
        source_line_count: usize,
        lines: Vec<String>,
    },
    Insert {
        op_index: usize,
        anchor_line: u32,
        insert_idx: usize,
        lines: Vec<String>,
    },
    Replace {
        op_index: usize,
        start_line: u32,
        end_line: u32,
        start_idx: usize,
        end_idx: usize,
        lines: Vec<String>,
    },
}

impl PlannedOp {
    fn op_index(&self) -> usize {
        match self {
            Self::Rewrite { op_index, .. }
            | Self::Insert { op_index, .. }
            | Self::Replace { op_index, .. } => *op_index,
        }
    }

    fn max_anchor_line(&self) -> u32 {
        match self {
            Self::Rewrite {
                source_line_count, ..
            } => u32::try_from(*source_line_count).unwrap_or(u32::MAX),
            Self::Insert { anchor_line, .. } => *anchor_line,
            Self::Replace { end_line, .. } => *end_line,
        }
    }

    fn changed_range(&self) -> ChangedLineRange {
        match self {
            Self::Rewrite {
                source_line_count,
                lines,
                ..
            } => ChangedLineRange {
                start_line: 1,
                removed_lines: u32::try_from(*source_line_count).unwrap_or(u32::MAX),
                added_lines: u32::try_from(lines.len()).unwrap_or(u32::MAX),
            },
            Self::Insert {
                insert_idx, lines, ..
            } => ChangedLineRange {
                start_line: u32::try_from(*insert_idx).unwrap_or(u32::MAX) + 1,
                removed_lines: 0,
                added_lines: u32::try_from(lines.len()).unwrap_or(u32::MAX),
            },
            Self::Replace {
                start_line,
                start_idx,
                end_idx,
                lines,
                ..
            } => ChangedLineRange {
                start_line: *start_line,
                removed_lines: u32::try_from(*end_idx - *start_idx).unwrap_or(u32::MAX),
                added_lines: u32::try_from(lines.len()).unwrap_or(u32::MAX),
            },
        }
    }

    fn conflicts_with(&self, other: &Self) -> bool {
        if matches!(self, Self::Rewrite { .. }) || matches!(other, Self::Rewrite { .. }) {
            return true;
        }

        match (self, other) {
            (
                Self::Insert {
                    anchor_line: left_anchor,
                    insert_idx: left_insert_idx,
                    ..
                },
                Self::Insert {
                    anchor_line: right_anchor,
                    insert_idx: right_insert_idx,
                    ..
                },
            ) => left_anchor == right_anchor || left_insert_idx == right_insert_idx,
            (
                Self::Insert { anchor_line, .. },
                Self::Replace {
                    start_line,
                    end_line,
                    ..
                },
            )
            | (
                Self::Replace {
                    start_line,
                    end_line,
                    ..
                },
                Self::Insert { anchor_line, .. },
            ) => *anchor_line >= *start_line && *anchor_line <= *end_line,
            (
                Self::Replace {
                    start_line: left_start,
                    end_line: left_end,
                    ..
                },
                Self::Replace {
                    start_line: right_start,
                    end_line: right_end,
                    ..
                },
            ) => left_start <= right_end && right_start <= left_end,
            _ => std::process::abort(),
        }
    }
}

struct SplitContent {
    lines: Vec<String>,
    trailing_newline: bool,
}

pub fn compute_line_hash(line: &str) -> String {
    let normalized = line.strip_suffix('\r').unwrap_or(line);
    blake3::hash(normalized.as_bytes())
        .to_hex()
        .chars()
        .take(HASH_HEX_LEN)
        .collect()
}

pub fn apply_hashline_patch(
    content: &str,
    patch: &HashlinePatch,
) -> Result<HashlineApplyResult, HashlineError> {
    if patch.ops.is_empty() {
        return Err(HashlineError::EmptyPatch {
            edit_id: patch.edit_id.clone(),
        });
    }

    let split = split_content(content);
    let mut planned_ops = Vec::with_capacity(patch.ops.len());

    for (op_index, op) in patch.ops.iter().enumerate() {
        planned_ops.push(plan_op(op, op_index, &split.lines)?);
    }

    detect_conflicts(&planned_ops)?;

    let mut execution_order = planned_ops.clone();
    execution_order.sort_by(|left, right| {
        right
            .max_anchor_line()
            .cmp(&left.max_anchor_line())
            .then_with(|| right.op_index().cmp(&left.op_index()))
    });

    let mut next_lines = split.lines;
    for planned in execution_order {
        match planned {
            PlannedOp::Rewrite { lines, .. } => {
                next_lines = lines;
            }
            PlannedOp::Insert {
                insert_idx, lines, ..
            } => {
                next_lines.splice(insert_idx..insert_idx, lines);
            }
            PlannedOp::Replace {
                start_idx,
                end_idx,
                lines,
                ..
            } => {
                next_lines.splice(start_idx..end_idx, lines);
            }
        }
    }

    let mut changed_ranges = planned_ops
        .iter()
        .map(PlannedOp::changed_range)
        .collect::<Vec<_>>();
    changed_ranges.sort_by_key(|range| range.start_line);

    Ok(HashlineApplyResult {
        content: join_content(&next_lines, split.trailing_newline),
        changed_ranges,
    })
}

fn plan_op(
    op: &HashlineOp,
    op_index: usize,
    source_lines: &[String],
) -> Result<PlannedOp, HashlineError> {
    match op {
        HashlineOp::Rewrite { lines } => Ok(PlannedOp::Rewrite {
            op_index,
            source_line_count: source_lines.len(),
            lines: lines.clone(),
        }),
        HashlineOp::InsertBefore { anchor, lines } => {
            validate_anchor(anchor, op_index, source_lines)?;
            Ok(PlannedOp::Insert {
                op_index,
                anchor_line: anchor.line,
                insert_idx: line_to_index(anchor.line, source_lines.len(), op_index)?,
                lines: lines.clone(),
            })
        }
        HashlineOp::InsertAfter { anchor, lines } => {
            validate_anchor(anchor, op_index, source_lines)?;
            Ok(PlannedOp::Insert {
                op_index,
                anchor_line: anchor.line,
                insert_idx: line_to_index(anchor.line, source_lines.len(), op_index)? + 1,
                lines: lines.clone(),
            })
        }
        HashlineOp::Replace { expected, lines } => {
            let (start_line, end_line) = validate_expected_block(expected, op_index, source_lines)?;
            let start_idx = line_to_index(start_line, source_lines.len(), op_index)?;
            let end_idx = line_to_index(end_line, source_lines.len(), op_index)? + 1;

            Ok(PlannedOp::Replace {
                op_index,
                start_line,
                end_line,
                start_idx,
                end_idx,
                lines: lines.clone(),
            })
        }
        HashlineOp::Delete { expected } => {
            let (start_line, end_line) = validate_expected_block(expected, op_index, source_lines)?;
            let start_idx = line_to_index(start_line, source_lines.len(), op_index)?;
            let end_idx = line_to_index(end_line, source_lines.len(), op_index)? + 1;

            Ok(PlannedOp::Replace {
                op_index,
                start_line,
                end_line,
                start_idx,
                end_idx,
                lines: Vec::new(),
            })
        }
    }
}

fn validate_anchor(
    anchor: &LineAnchor,
    op_index: usize,
    source_lines: &[String],
) -> Result<(), HashlineError> {
    let index = line_to_index(anchor.line, source_lines.len(), op_index)?;
    let actual_hash = compute_line_hash(&source_lines[index]);

    if actual_hash != anchor.hash {
        return Err(HashlineError::AnchorMismatch {
            op_index,
            line: anchor.line,
            expected_hash: anchor.hash.clone(),
            actual_hash,
        });
    }

    Ok(())
}

fn validate_expected_block(
    expected: &[LineAnchor],
    op_index: usize,
    source_lines: &[String],
) -> Result<(u32, u32), HashlineError> {
    if expected.is_empty() {
        return Err(HashlineError::OutOfRange {
            op_index,
            line: 0,
            max_line: u32::try_from(source_lines.len()).unwrap_or(u32::MAX),
        });
    }

    for anchor in expected {
        validate_anchor(anchor, op_index, source_lines)?;
    }

    for pair in expected.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        if right.line != left.line + 1 {
            return Err(HashlineError::Overlap {
                first_op_index: op_index,
                second_op_index: op_index,
                reason: "replace/delete anchors must be contiguous and ordered".to_string(),
            });
        }
    }

    Ok((expected[0].line, expected[expected.len() - 1].line))
}

fn line_to_index(line: u32, source_len: usize, op_index: usize) -> Result<usize, HashlineError> {
    if line == 0 || line > u32::try_from(source_len).unwrap_or(u32::MAX) {
        return Err(HashlineError::OutOfRange {
            op_index,
            line,
            max_line: u32::try_from(source_len).unwrap_or(u32::MAX),
        });
    }

    Ok((line - 1) as usize)
}

fn detect_conflicts(planned_ops: &[PlannedOp]) -> Result<(), HashlineError> {
    for left_idx in 0..planned_ops.len() {
        for right_idx in (left_idx + 1)..planned_ops.len() {
            let left = &planned_ops[left_idx];
            let right = &planned_ops[right_idx];

            if left.conflicts_with(right) {
                return Err(HashlineError::Overlap {
                    first_op_index: left.op_index(),
                    second_op_index: right.op_index(),
                    reason: "operations target overlapping/conflicting lines".to_string(),
                });
            }
        }
    }

    Ok(())
}

fn split_content(content: &str) -> SplitContent {
    if content.is_empty() {
        return SplitContent {
            lines: Vec::new(),
            trailing_newline: false,
        };
    }

    let trailing_newline = content.ends_with('\n');
    let body = if trailing_newline {
        &content[..content.len() - 1]
    } else {
        content
    };

    SplitContent {
        lines: body.split('\n').map(ToString::to_string).collect(),
        trailing_newline,
    }
}

fn join_content(lines: &[String], trailing_newline: bool) -> String {
    if lines.is_empty() {
        return String::new();
    }

    let mut content = lines.join("\n");
    if trailing_newline {
        content.push('\n');
    }

    content
}

#[cfg(test)]
mod tests {
    use super::{
        apply_hashline_patch, compute_line_hash, join_content, split_content, ChangedLineRange,
        HashlineError, HashlineOp, HashlinePatch, LineAnchor,
    };
    use crate::UnwrapOrAbort;
    use proptest::prelude::*;
    use proptest::test_runner::Config as ProptestConfig;

    #[test]
    fn hashline_compute_line_hash_normalizes_crlf_lf() {
        // arrange
        // act
        // assert
        assert_eq!(compute_line_hash("line"), compute_line_hash("line\r"));
    }

    #[test]
    fn hashline_compute_line_hash_handles_unicode_tabs_and_empty_lines() {
        // arrange
        // act
        // assert
        let unicode_hash = compute_line_hash("snow☃\t東京");
        assert_eq!(unicode_hash.len(), 12);
        assert!(unicode_hash.chars().all(|c| c.is_ascii_hexdigit()));

        assert_eq!(compute_line_hash(""), compute_line_hash("\r"));
        assert_ne!(compute_line_hash("\t"), compute_line_hash("    "));
    }

    #[test]
    fn hashline_golden_small_file_edits() {
        // arrange
        // act
        // assert
        let original = "alpha\nbeta\ngamma\ndelta\n";
        let lines = split_content(original).lines;

        let patch = HashlinePatch {
            edit_id: "edit-11-golden".to_string(),
            path: "demo.txt".to_string(),
            ops: vec![
                HashlineOp::InsertBefore {
                    anchor: anchor_for(&lines, 2),
                    lines: vec!["intro".to_string()],
                },
                HashlineOp::Replace {
                    expected: vec![anchor_for(&lines, 3)],
                    lines: vec!["GAMMA".to_string()],
                },
                HashlineOp::Delete {
                    expected: vec![anchor_for(&lines, 4)],
                },
            ],
        };

        let result = apply_hashline_patch(original, &patch).unwrap_or_abort();

        assert_eq!(result.content, "alpha\nintro\nbeta\nGAMMA\n");
        assert_eq!(
            result.changed_ranges,
            vec![
                ChangedLineRange {
                    start_line: 2,
                    removed_lines: 0,
                    added_lines: 1,
                },
                ChangedLineRange {
                    start_line: 3,
                    removed_lines: 1,
                    added_lines: 1,
                },
                ChangedLineRange {
                    start_line: 4,
                    removed_lines: 1,
                    added_lines: 0,
                },
            ]
        );
    }

    #[test]
    fn hashline_empty_patch_rejected() {
        // arrange
        // act
        // assert
        let patch = HashlinePatch {
            edit_id: "empty".to_string(),
            path: "demo.txt".to_string(),
            ops: Vec::new(),
        };

        let error = apply_hashline_patch("alpha\n", &patch).expect_err("empty patch must fail");
        assert_eq!(error.code(), "EMPTY_PATCH");
        assert!(matches!(error, HashlineError::EmptyPatch { .. }));
    }

    #[test]
    fn hashline_rewrite_op_replaces_entire_file() {
        // arrange
        // act
        // assert
        let patch = HashlinePatch {
            edit_id: "rewrite".to_string(),
            path: "demo.txt".to_string(),
            ops: vec![HashlineOp::Rewrite {
                lines: vec!["fresh".to_string(), "content".to_string()],
            }],
        };

        let applied = apply_hashline_patch("alpha\nbeta\n", &patch).unwrap_or_abort();
        assert_eq!(applied.content, "fresh\ncontent\n");
        assert_eq!(
            applied.changed_ranges,
            vec![ChangedLineRange {
                start_line: 1,
                removed_lines: 2,
                added_lines: 2,
            }]
        );
    }

    #[test]
    fn hashline_rewrite_conflicts_with_other_ops() {
        // arrange
        // act
        // assert
        let patch = HashlinePatch {
            edit_id: "rewrite-overlap".to_string(),
            path: "demo.txt".to_string(),
            ops: vec![
                HashlineOp::Rewrite {
                    lines: vec!["fresh".to_string()],
                },
                HashlineOp::InsertBefore {
                    anchor: LineAnchor {
                        line: 1,
                        hash: compute_line_hash("alpha"),
                    },
                    lines: vec!["intro".to_string()],
                },
            ],
        };

        let error = apply_hashline_patch("alpha\n", &patch).expect_err("must reject conflict");
        assert_eq!(error.code(), "OVERLAP");
    }

    #[test]
    fn hashline_out_of_range_anchor_rejected() {
        // arrange
        // act
        // assert
        let patch = HashlinePatch {
            edit_id: "out-of-range".to_string(),
            path: "demo.txt".to_string(),
            ops: vec![HashlineOp::InsertBefore {
                anchor: LineAnchor {
                    line: 42,
                    hash: "000000000000".to_string(),
                },
                lines: vec!["x".to_string()],
            }],
        };

        let error = apply_hashline_patch("alpha\nbeta\n", &patch).expect_err("must fail");
        assert_eq!(error.code(), "OUT_OF_RANGE");
        assert!(matches!(error, HashlineError::OutOfRange { .. }));
    }

    #[test]
    fn hashline_overlapping_ops_rejected() {
        // arrange
        // act
        // assert
        let original = "alpha\nbeta\ngamma\ndelta\n";
        let lines = split_content(original).lines;

        let patch = HashlinePatch {
            edit_id: "overlap".to_string(),
            path: "demo.txt".to_string(),
            ops: vec![
                HashlineOp::Replace {
                    expected: vec![anchor_for(&lines, 2), anchor_for(&lines, 3)],
                    lines: vec!["BETA".to_string(), "GAMMA".to_string()],
                },
                HashlineOp::Delete {
                    expected: vec![anchor_for(&lines, 3)],
                },
            ],
        };

        let error = apply_hashline_patch(original, &patch).expect_err("must fail");
        assert_eq!(error.code(), "OVERLAP");
        assert!(matches!(error, HashlineError::Overlap { .. }));
    }

    #[test]
    fn hashline_anchor_mismatch_rejected() {
        // arrange
        // act
        // assert
        let original = "alpha\nbeta\ngamma\n";
        let lines = split_content(original).lines;

        let mut bad_anchor = anchor_for(&lines, 2);
        bad_anchor.hash = "ffffffffffff".to_string();

        let patch = HashlinePatch {
            edit_id: "mismatch".to_string(),
            path: "demo.txt".to_string(),
            ops: vec![HashlineOp::InsertAfter {
                anchor: bad_anchor,
                lines: vec!["inserted".to_string()],
            }],
        };

        let error = apply_hashline_patch(original, &patch).expect_err("must fail");
        assert_eq!(error.code(), "ANCHOR_MISMATCH");
        assert!(matches!(error, HashlineError::AnchorMismatch { .. }));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn hashline_prop_non_overlapping_ops_are_atomic(
            base_lines in prop::collection::vec(line_strategy(), 1..20),
            picks in prop::collection::vec(0usize..80, 1..20),
            trailing_newline in any::<bool>(),
        ) {
            // arrange
            // act
            // assert
            let original = join_content(&base_lines, trailing_newline);
            let split = split_content(&original);
            let source_lines = split.lines;
            let source_trailing_newline = split.trailing_newline;
            let line_count = source_lines.len();
            prop_assume!(line_count > 0);

            let mut selected_line_numbers = picks
                .into_iter()
                .map(|index| (index % line_count) + 1)
                .collect::<Vec<_>>();
            selected_line_numbers.sort_unstable();
            selected_line_numbers.dedup();

            let mut ops = Vec::with_capacity(selected_line_numbers.len());
            let mut model = Vec::with_capacity(selected_line_numbers.len());

            for (position, line_number) in selected_line_numbers.into_iter().enumerate() {
                let line_index = line_number - 1;
                let anchor = LineAnchor {
                    line: u32::try_from(line_number).unwrap_or(u32::MAX),
                    hash: compute_line_hash(&source_lines[line_index]),
                };

                if position % 2 == 0 {
                    ops.push(HashlineOp::Delete {
                        expected: vec![anchor],
                    });
                    model.push((line_index, None));
                } else {
                    let replacement = format!("{}_new", base_lines[line_index]);
                    ops.push(HashlineOp::Replace {
                        expected: vec![anchor],
                        lines: vec![replacement.clone()],
                    });
                    model.push((line_index, Some(replacement)));
                }
            }

            let patch = HashlinePatch {
                edit_id: "prop".to_string(),
                path: "prop.txt".to_string(),
                ops: ops.clone(),
            };

            let applied = apply_hashline_patch(&original, &patch)
                .unwrap_or_abort();

            let mut expected_lines = source_lines.clone();
            model.sort_by_key(|entry| std::cmp::Reverse(entry.0));
            for (line_index, maybe_replacement) in model {
                match maybe_replacement {
                    Some(replacement) => {
                        expected_lines[line_index] = replacement;
                    }
                    None => {
                        expected_lines.remove(line_index);
                    }
                }
            }
            let expected = join_content(&expected_lines, source_trailing_newline);
            prop_assert_eq!(applied.content, expected.clone());

            let mut mismatched_patch = patch.clone();
            corrupt_first_anchor(&mut mismatched_patch);
            let mismatch = apply_hashline_patch(&original, &mismatched_patch)
                .expect_err("mismatch must fail atomically");
            prop_assert_eq!(mismatch.code(), "ANCHOR_MISMATCH");

            let reapplied = apply_hashline_patch(&original, &patch)
                .unwrap_or_abort();
            prop_assert_eq!(reapplied.content, expected);
        }
    }

    fn anchor_for(lines: &[String], line: u32) -> LineAnchor {
        let index = (line - 1) as usize;
        LineAnchor {
            line,
            hash: compute_line_hash(&lines[index]),
        }
    }

    fn line_strategy() -> impl Strategy<Value = String> {
        proptest::string::string_regex("[a-zA-Z0-9 \\t]{0,16}").unwrap_or_abort()
    }

    fn corrupt_first_anchor(patch: &mut HashlinePatch) {
        let replacement_hash = |hash: &str| {
            if hash == "000000000000" {
                "ffffffffffff".to_string()
            } else {
                "000000000000".to_string()
            }
        };

        match patch.ops.first_mut().unwrap_or_abort() {
            HashlineOp::Rewrite { .. } => {
                panic!("rewrite ops cannot be anchor-corrupted for mismatch checks")
            }
            HashlineOp::InsertBefore { anchor, .. } | HashlineOp::InsertAfter { anchor, .. } => {
                anchor.hash = replacement_hash(&anchor.hash);
            }
            HashlineOp::Replace { expected, .. } | HashlineOp::Delete { expected } => {
                let first = expected.first_mut().unwrap_or_abort();
                first.hash = replacement_hash(&first.hash);
            }
        }
    }
}

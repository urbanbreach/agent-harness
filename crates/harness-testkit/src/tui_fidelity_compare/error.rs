use std::fmt;
use std::path::PathBuf;

use super::cells::CellDiffRecord;
use super::hashing::StaleArtifact;
use super::motion::MotionIssue;
use super::pixels::PixelDiffRecord;
use super::secret_scan::SecretFinding;
use super::timing::TimingDefect;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComparatorError {
    Cells {
        diffs: Vec<CellDiffRecord>,
        diffs_len: usize,
    },
    Pixels {
        diffs: Vec<PixelDiffRecord>,
        diffs_len: usize,
    },
    Motion {
        defects: Vec<MotionIssue>,
        defects_len: usize,
    },
    Timing {
        defects: Vec<TimingDefect>,
        defects_len: usize,
    },
    Hashing {
        stale: Vec<StaleArtifact>,
        stale_len: usize,
    },
    SelfComparison {
        sha256: String,
    },
    Secrets {
        findings: Vec<SecretFinding>,
        findings_len: usize,
    },
    PngDecode {
        side: String,
        detail: String,
    },
    Invalid {
        detail: String,
    },
    Io {
        path: PathBuf,
        detail: String,
    },
}

impl fmt::Display for ComparatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cells { diffs_len, .. } => {
                write!(
                    formatter,
                    "semantic comparison found {diffs_len} differences"
                )
            }
            Self::Pixels { diffs_len, .. } => {
                write!(formatter, "pixel comparison found {diffs_len} differences")
            }
            Self::Motion { defects_len, .. } => {
                write!(formatter, "motion comparison found {defects_len} defects")
            }
            Self::Timing { defects_len, .. } => {
                write!(formatter, "timing comparison found {defects_len} defects")
            }
            Self::Hashing { stale_len, .. } => {
                write!(
                    formatter,
                    "artifact freshness failed for {stale_len} artifact(s)"
                )
            }
            Self::SelfComparison { sha256 } => {
                write!(
                    formatter,
                    "reference and candidate share binary SHA-256 {sha256}"
                )
            }
            Self::Secrets { findings_len, .. } => {
                write!(formatter, "secret scan found {findings_len} token(s)")
            }
            Self::PngDecode { side, detail } => {
                write!(formatter, "PNG decode failed for {side}: {detail}")
            }
            Self::Invalid { detail } => write!(formatter, "invalid comparator input: {detail}"),
            Self::Io { path, detail } => {
                write!(formatter, "I/O error for {}: {detail}", path.display())
            }
        }
    }
}

impl std::error::Error for ComparatorError {}

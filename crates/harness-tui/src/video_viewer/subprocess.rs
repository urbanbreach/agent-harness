use super::lifecycle::ViewerError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubprocessDescriptor {
    pub binary: String,
    pub args: Vec<String>,
    pub max_duration_ms: u64,
    pub max_width: u32,
    pub max_height: u32,
}

pub fn sanitize_args(args: &[String]) -> Result<Vec<String>, ViewerError> {
    args.iter()
        .map(|arg| {
            if arg.is_empty() {
                return Err(ViewerError::MalformedArg(arg.clone()));
            }
            if arg.chars().any(|character| {
                matches!(
                    character,
                    ';' | '|' | '&' | '$' | '`' | '\n' | '\r' | '>' | '<'
                )
            }) {
                return Err(ViewerError::MalformedArg(arg.clone()));
            }
            Ok(arg.clone())
        })
        .collect()
}

impl SubprocessDescriptor {
    pub fn validate(&self) -> Result<(), ViewerError> {
        if !matches!(self.binary.as_str(), "ffmpeg" | "ffprobe") {
            return Err(ViewerError::UnknownBinary);
        }
        sanitize_args(&self.args)?;
        if self.max_duration_ms == 0
            || self.max_duration_ms > 600_000
            || self.max_width == 0
            || self.max_width > 7680
            || self.max_height == 0
            || self.max_height > 7680
        {
            return Err(ViewerError::OversizedMedia);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubprocessReceipt {
    pub descriptor: SubprocessDescriptor,
    pub exit_code: Option<i32>,
    pub completed_normally: bool,
    pub temp_files_created: Vec<String>,
    pub temp_files_removed: Vec<String>,
    pub child_pids_observed: Vec<u32>,
    pub child_pids_reaped: Vec<u32>,
}

impl SubprocessReceipt {
    pub fn cleanup_complete(&self) -> bool {
        self.temp_files_created
            .iter()
            .all(|path| self.temp_files_removed.contains(path))
            && self
                .child_pids_observed
                .iter()
                .all(|pid| self.child_pids_reaped.contains(pid))
    }
}

pub struct SubprocessSupervisor {
    descriptors: Vec<SubprocessDescriptor>,
}

impl Default for SubprocessSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl SubprocessSupervisor {
    pub const fn new() -> Self {
        Self {
            descriptors: Vec::new(),
        }
    }

    pub fn submit(&mut self, descriptor: SubprocessDescriptor) -> Result<usize, ViewerError> {
        descriptor.validate()?;
        let index = self.descriptors.len();
        self.descriptors.push(descriptor);
        Ok(index)
    }

    pub fn simulate_run(
        &self,
        index: usize,
        exit_code: Option<i32>,
    ) -> Result<SubprocessReceipt, ViewerError> {
        let descriptor = self
            .descriptors
            .get(index)
            .cloned()
            .ok_or(ViewerError::UnknownRequest)?;
        let completed_normally = exit_code == Some(0);
        let (temp_files_created, temp_files_removed, child_pids_observed, child_pids_reaped) =
            if completed_normally {
                (Vec::new(), Vec::new(), Vec::new(), Vec::new())
            } else {
                (
                    vec![format!("video-viewer-{index}.tmp")],
                    Vec::new(),
                    vec![u32::try_from(index).unwrap_or(u32::MAX)],
                    Vec::new(),
                )
            };
        Ok(SubprocessReceipt {
            descriptor,
            exit_code,
            completed_normally,
            temp_files_created,
            temp_files_removed,
            child_pids_observed,
            child_pids_reaped,
        })
    }

    pub const fn len(&self) -> usize {
        self.descriptors.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }

    pub fn clear(&mut self) {
        self.descriptors.clear();
    }
}

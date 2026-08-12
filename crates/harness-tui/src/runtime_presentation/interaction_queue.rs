use std::fs;
use std::io::{self, BufRead, Seek};
use std::num::NonZeroU16;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::presentation::InteractionId;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionEventClass {
    Key,
    Paste,
    Mouse,
    Resize,
    Focus,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueuedInteraction {
    interaction_id: InteractionId,
    event_class: InteractionEventClass,
    receipt_count: NonZeroU16,
    #[serde(skip)]
    attributed: bool,
}

#[derive(Debug)]
pub(super) struct InteractionQueue {
    path: PathBuf,
    offset: u64,
    pending: Option<QueuedInteraction>,
}

impl InteractionQueue {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            pending: None,
        }
    }

    pub(super) fn take(
        &mut self,
        event_class: InteractionEventClass,
    ) -> io::Result<Option<InteractionId>> {
        if self.pending.is_none() {
            self.pending = self.read_next()?;
        }
        let Some(pending) = self.pending.as_mut() else {
            return Ok(None);
        };
        if pending.event_class != event_class {
            return Ok(None);
        }

        let interaction_id = (!pending.attributed).then(|| pending.interaction_id.clone());
        pending.attributed = true;
        let remaining = pending.receipt_count.get() - 1;
        if remaining == 0 {
            self.pending = None;
        } else {
            pending.receipt_count = NonZeroU16::new(remaining).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "interaction receipt count is zero",
                )
            })?;
        }
        Ok(interaction_id)
    }

    fn read_next(&mut self) -> io::Result<Option<QueuedInteraction>> {
        let mut file = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        file.seek(io::SeekFrom::Start(self.offset))?;
        let mut reader = io::BufReader::new(file);
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 || !line.ends_with('\n') {
            return Ok(None);
        }
        let queued = serde_json::from_str(line.trim_end_matches(['\r', '\n']))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        self.offset = self
            .offset
            .saturating_add(u64::try_from(bytes_read).unwrap_or(u64::MAX));
        Ok(Some(queued))
    }
}

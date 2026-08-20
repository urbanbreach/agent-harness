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
    Wheel,
    Resize,
    Focus,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueuedInteraction {
    interaction_id: InteractionId,
    event_class: InteractionEventClass,
    receipt_count: NonZeroU16,
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

        let interaction_id = pending.interaction_id.clone();
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
        Ok(Some(interaction_id))
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::{InteractionEventClass, InteractionQueue};
    use crate::presentation::InteractionId;

    #[test]
    fn single_receipt_is_attributed_once() {
        // arrange
        let (file, mut queue) = queue("single", "mouse", 1);

        // act
        let receipt = queue.take(InteractionEventClass::Mouse).expect("receipt");
        let exhausted = queue.take(InteractionEventClass::Mouse).expect("exhausted");

        // assert
        assert_eq!(receipt, Some(InteractionId::new("single")));
        assert_eq!(exhausted, None);
        drop(file);
    }

    #[test]
    fn click_down_and_up_share_the_declared_interaction_id() {
        // arrange
        let (file, mut queue) = queue("click", "mouse", 2);

        // act
        let down = queue.take(InteractionEventClass::Mouse).expect("down");
        let up = queue.take(InteractionEventClass::Mouse).expect("up");
        let excess = queue.take(InteractionEventClass::Mouse).expect("excess");

        // assert
        assert_eq!(down, Some(InteractionId::new("click")));
        assert_eq!(up, Some(InteractionId::new("click")));
        assert_eq!(
            excess, None,
            "a third mouse receipt must not inherit an exhausted interaction"
        );
        drop(file);
    }

    #[test]
    fn wrong_event_class_does_not_consume_the_pending_receipt() {
        // arrange
        let (file, mut queue) = queue("click", "mouse", 2);

        // act
        let wrong_class = queue.take(InteractionEventClass::Key).expect("wrong class");
        let down = queue.take(InteractionEventClass::Mouse).expect("down");
        let up = queue.take(InteractionEventClass::Mouse).expect("up");

        // assert
        assert_eq!(wrong_class, None);
        assert_eq!(down, Some(InteractionId::new("click")));
        assert_eq!(up, Some(InteractionId::new("click")));
        drop(file);
    }

    fn queue(
        interaction_id: &str,
        event_class: &str,
        receipt_count: u16,
    ) -> (tempfile::NamedTempFile, InteractionQueue) {
        let mut file = tempfile::NamedTempFile::new().expect("temporary interaction queue");
        writeln!(
            file,
            "{{\"interaction_id\":\"{interaction_id}\",\"event_class\":\"{event_class}\",\"receipt_count\":{receipt_count}}}"
        )
        .expect("write interaction queue");
        file.flush().expect("flush interaction queue");
        let queue = InteractionQueue::new(file.path().to_path_buf());
        (file, queue)
    }
}

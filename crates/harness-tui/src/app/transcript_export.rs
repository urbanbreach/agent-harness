use std::fmt::Write as _;

use super::transcript_outline::{activity_blocks, replay_turn};
use super::AppState;
use crate::transcript_blocks::{default_fold, BlockSnapshot, RawDisclosure};
use crate::transcript_pager::{
    run_pager, PagerCommand, PagerError, PagerExit, PagerStdio, TerminalControl, TranscriptSnapshot,
};

impl AppState {
    pub fn run_transcript_pager<T: TerminalControl>(
        &mut self,
        pager_command: &PagerCommand,
        stdio: PagerStdio,
        terminal: &mut T,
    ) -> Result<PagerExit, PagerError> {
        let mut text = String::new();
        for block in self.activities.iter().flat_map(activity_blocks) {
            let _ = writeln!(text, "{}\n{}", block.kind.as_str(), block.content);
        }
        run_pager(
            &TranscriptSnapshot::from_text(&text),
            pager_command,
            stdio,
            terminal,
        )
    }

    pub(super) fn transcript_blocks(&self) -> Vec<BlockSnapshot> {
        self.activities
            .iter()
            .enumerate()
            .flat_map(|(index, activity)| {
                let replay = replay_turn(index, activity, 0);
                activity_blocks(activity)
                    .enumerate()
                    .map(move |(index, block)| BlockSnapshot {
                        id: replay.block_id(u64::try_from(index).unwrap_or(u64::MAX)),
                        kind: block.kind,
                        lifecycle: block.lifecycle,
                        content: block.content.to_owned(),
                        fold_state: default_fold(block.kind, block.lifecycle),
                        raw: block.raw.map(RawDisclosure::from_json),
                    })
            })
            .collect()
    }
}

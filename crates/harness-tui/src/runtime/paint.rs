use super::Runtime;
use crate::{
    terminal::{FrameKind, FrameSubmission},
    ui,
};
use anyhow::Result;
use std::time::Instant;

impl Runtime {
    pub(super) fn finish_unchanged_demand(&mut self) -> Result<()> {
        if let Some(trace) = self.trace.as_mut() {
            if let Some(demand) = trace.take_render_demand() {
                trace.record_no_visible_change(&demand)?;
            }
        }
        Ok(())
    }

    pub(super) fn paint(&mut self) -> Result<()> {
        let demand = self
            .trace
            .as_mut()
            .and_then(|trace| trace.take_render_demand());
        self.frame_at = None;
        let urgent = std::mem::take(&mut self.urgent);
        if self.trace.is_some() && demand.is_none() {
            self.live_turn |= urgent;
            return Ok(());
        }
        let area = self.frame_area()?;
        self.app.set_frame_area(area);
        self.experience.tick();
        let kind = match demand.as_ref() {
            Some(demand) => self.output.begin_frame_for(demand.clone())?,
            None => self.output.begin_frame()?,
        };
        let rendered = (|| -> Result<()> {
            if kind == FrameKind::FullRepaint {
                self.terminal.backend_mut().invalidate_cursor_state();
                self.terminal.clear()?;
            }
            self.terminal
                .backend_mut()
                .set_hyperlinks(std::mem::take(&mut self.app.transcript_view.hyperlinks));
            self.terminal
                .draw(|frame| ui::render_app(frame, &self.app))?;
            self.experience.post_flush(self.terminal.backend_mut());
            Ok(())
        })();
        if let Err(error) = rendered {
            self.output.abort_frame();
            return Err(error);
        }
        let submission = self.output.finish_frame()?;
        if submission == FrameSubmission::ResyncRequired {
            self.request_frame(urgent, Instant::now());
        } else {
            self.live_turn |= urgent;
        }
        if let (Some(trace), Some(demand)) = (self.trace.as_mut(), demand.as_ref()) {
            match submission {
                FrameSubmission::Accepted(_) => {}
                FrameSubmission::Unchanged => trace.record_no_visible_change(demand)?,
                FrameSubmission::ResyncRequired => trace.record_resync(demand)?,
            }
        }
        Ok(())
    }
}

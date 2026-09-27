use harness_core::{
    event::{EventEnvelopeV1, EventV1, LiveEventEnvelope, LiveEventV1},
    session::AssistantPart,
};
use std::io::Write;

#[derive(Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub(super) enum Format {
    #[default]
    Default,
    Json,
    StreamingJson,
}
pub(super) struct Output<'a> {
    writer: &'a mut dyn Write,
    format: Format,
    first: bool,
    text: StreamText,
    thinking: bool,
    reasoning_section: bool,
    live_enabled: bool,
}
impl<'a> Output<'a> {
    pub fn new(writer: &'a mut dyn Write, format: Format, thinking: bool) -> Self {
        Self {
            writer,
            format,
            first: true,
            thinking,
            text: StreamText::default(),
            reasoning_section: false,
            live_enabled: true,
        }
    }
    pub fn event(&mut self, event: &EventEnvelopeV1, task: &str) -> std::io::Result<()> {
        if event.correlation_id.as_deref() != Some(task)
            && !matches!(event.payload, EventV1::RunFailed(_))
        {
            return Ok(());
        }
        if matches!(event.payload, EventV1::ProviderRequestFinished(_)) {
            self.end_reasoning()?;
        }
        if self.format == Format::Default {
            let EventV1::AssistantMessageFinished(message) = &event.payload else {
                return Ok(());
            };
            for part in &message.parts {
                if let AssistantPart::Text { text } = part {
                    self.finish_text(text)?;
                }
            }
        } else {
            self.json("durable", event)?;
            if matches!(event.payload, EventV1::AssistantMessageFinished(_)) {
                self.text = StreamText::default();
            }
        }
        self.writer.flush()
    }
    fn finish_text(&mut self, text: &str) -> std::io::Result<()> {
        self.end_reasoning()?;
        let prefix = text.get(..self.text.written);
        let remaining = if prefix
            .is_some_and(|prefix| blake3::hash(prefix.as_bytes()) == self.text.hash.finalize())
        {
            &text[self.text.written..]
        } else {
            if self.text.written != 0 {
                writeln!(self.writer)?;
            }
            text
        };
        writeln!(self.writer, "{remaining}")?;
        self.text = StreamText::default();
        Ok(())
    }
    pub fn live(&mut self, event: LiveEventEnvelope, task: &str) -> std::io::Result<()> {
        if !self.live_enabled || event.correlation_id.as_deref() != Some(task) {
            return Ok(());
        }
        let (thinking, delta) = match &event.payload {
            LiveEventV1::ProviderTextDelta { delta, .. } => (false, delta),
            LiveEventV1::ProviderReasoningDelta { delta, .. } if self.thinking => (true, delta),
            _ => return Ok(()),
        };
        if delta.is_empty() {
            return Ok(());
        }
        // The coordinator publishes only safe fragments; track their committed prefix.
        if !thinking {
            self.text.written += delta.len();
            self.text.hash.update(delta.as_bytes());
        }
        self.fragment(thinking, delta)?;
        if self.format != Format::Default {
            self.json("live", &event)?;
        }
        self.writer.flush()
    }
    pub fn lagged(&mut self) {
        self.live_enabled = false;
    }
    fn fragment(&mut self, thinking: bool, text: &str) -> std::io::Result<()> {
        if self.format != Format::Default {
            return Ok(());
        }
        if thinking && !self.reasoning_section {
            write!(self.writer, "\n[thinking]\n")?;
            self.reasoning_section = true;
        } else if !thinking {
            self.end_reasoning()?;
        }
        self.writer.write_all(text.as_bytes())
    }
    fn end_reasoning(&mut self) -> std::io::Result<()> {
        if self.reasoning_section {
            writeln!(self.writer)?;
            self.reasoning_section = false;
        }
        Ok(())
    }
    fn json(&mut self, delivery: &str, event: &impl serde::Serialize) -> std::io::Result<()> {
        if self.format == Format::Json {
            self.writer
                .write_all(if self.first { b"[" } else { b"," })?;
        }
        self.first = false;
        write!(self.writer, "{{\"delivery\":\"{delivery}\",\"event\":")?;
        serde_json::to_writer(&mut self.writer, event)?;
        self.writer.write_all(b"}")?;
        if self.format == Format::StreamingJson {
            writeln!(self.writer)?;
        }
        Ok(())
    }
    pub fn finish(&mut self) -> std::io::Result<()> {
        if self.format == Format::Json {
            self.writer
                .write_all(if self.first { b"[]\n" } else { b"]\n" })?;
        }
        self.writer.flush()
    }
}

#[derive(Default)]
struct StreamText {
    written: usize,
    hash: blake3::Hasher,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lagged_output_uses_the_committed_answer_without_repeating_its_prefix(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let live = |delta: &str| -> Result<LiveEventEnvelope, serde_json::Error> {
            serde_json::from_value(serde_json::json!({
                "event_id":"live", "run_id":"run", "mono_ms":0,
                "actor":{"kind":"worker", "agent_id":"agent"}, "correlation_id":"turn",
                "payload":{"event_type":"provider_text_delta", "data":{"request_id":"provider", "delta":delta}}
            }))
        };
        let mut bytes = Vec::new();
        let mut output = Output::new(&mut bytes, Format::Default, false);
        output.live(live("Visible prefix ")?, "turn")?;
        output.lagged();
        output.live(live("missing intermediate text ")?, "turn")?;
        let mut finished = serde_json::to_value(live("")?)?;
        finished["schema_version"] = 1.into();
        finished["seq"] = 1.into();
        finished["payload"] = serde_json::to_value(EventV1::AssistantMessageFinished(
            harness_core::event::AssistantMessageFinishedEvent {
                request_id: "provider".into(),
                tool_call_count: 0,
                parts: vec![AssistantPart::Text {
                    text: "Visible prefix and committed ending".into(),
                }],
                provenance: None,
                assistant_message: None,
            },
        ))?;
        output.event(&serde_json::from_value(finished)?, "turn")?;
        output.finish()?;
        assert_eq!(
            String::from_utf8(bytes)?,
            "Visible prefix and committed ending\n"
        );
        Ok(())
    }
}

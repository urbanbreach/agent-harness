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
    thinking: bool,
    reasoning_section: bool,
    live_enabled: bool,
    /// Default format prints only the last tool-free assistant message, like print modes in
    /// other harnesses: opening lines and narration from tool-calling messages stay out of stdout.
    answer: Option<String>,
}
impl<'a> Output<'a> {
    pub fn new(writer: &'a mut dyn Write, format: Format, thinking: bool) -> Self {
        Self {
            writer,
            format,
            first: true,
            thinking,
            reasoning_section: false,
            live_enabled: true,
            answer: None,
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
            if message.tool_call_count == 0 {
                let text: Vec<&str> = message
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        AssistantPart::Text { text } if !text.trim().is_empty() => {
                            Some(text.as_str())
                        }
                        _ => None,
                    })
                    .collect();
                if !text.is_empty() {
                    self.answer = Some(text.join("\n"));
                }
            }
        } else {
            self.json("durable", event)?;
        }
        self.writer.flush()
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
        if self.format == Format::Default {
            if thinking {
                self.reasoning(delta)?;
            }
        } else {
            self.json("live", &event)?;
        }
        self.writer.flush()
    }
    pub fn lagged(&mut self) {
        self.live_enabled = false;
    }
    fn reasoning(&mut self, text: &str) -> std::io::Result<()> {
        if !self.reasoning_section {
            write!(self.writer, "\n[thinking]\n")?;
            self.reasoning_section = true;
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
        self.end_reasoning()?;
        if let Some(answer) = self.answer.take() {
            writeln!(self.writer, "{answer}")?;
        }
        if self.format == Format::Json {
            self.writer
                .write_all(if self.first { b"[]\n" } else { b"]\n" })?;
        }
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_output_prints_only_the_last_tool_free_message(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let event = |payload: serde_json::Value| -> Result<serde_json::Value, serde_json::Error> {
            Ok(serde_json::json!({
                "schema_version":1, "seq":1, "event_id":"e", "run_id":"run", "mono_ms":0,
                "actor":{"kind":"worker", "agent_id":"agent"}, "correlation_id":"turn",
                "payload":payload
            }))
        };
        let message = |tool_call_count: usize, text: &str| {
            event(
                serde_json::json!({"event_type":"assistant_message_finished", "data":{
                    "request_id":"provider", "tool_call_count":tool_call_count,
                    "parts":[{"kind":"text", "text":text}]
                }}),
            )
        };
        let mut bytes = Vec::new();
        let mut output = Output::new(&mut bytes, Format::Default, false);
        let delta = serde_json::json!({
            "event_id":"live", "run_id":"run", "mono_ms":0,
            "actor":{"kind":"worker", "agent_id":"agent"}, "correlation_id":"turn",
            "payload":{"event_type":"provider_text_delta", "data":{"request_id":"provider", "delta":"Intent: read the file"}}
        });
        output.live(serde_json::from_value(delta)?, "turn")?;
        output.event(
            &serde_json::from_value(message(1, "Intent: read the file. Reading it now.")?)?,
            "turn",
        )?;
        output.event(
            &serde_json::from_value(message(0, "Draft answer")?)?,
            "turn",
        )?;
        output.event(
            &serde_json::from_value(message(0, "Final answer")?)?,
            "turn",
        )?;
        output.finish()?;
        assert_eq!(String::from_utf8(bytes)?, "Final answer\n");
        Ok(())
    }
}

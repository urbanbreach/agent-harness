#[derive(Debug, Default)]
pub(crate) struct Decoder {
    line: Vec<u8>,
    data: String,
    has_data: bool,
    skip_lf: bool,
    started: bool,
}

const MAX_EVENT_BYTES: usize = 1_048_576;

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, &'static str> {
        let mut frames = Vec::new();
        for &byte in bytes {
            if std::mem::take(&mut self.skip_lf) && byte == b'\n' {
                continue;
            }
            if byte != b'\r' && byte != b'\n' {
                if self.line.len() + self.data.len() >= MAX_EVENT_BYTES {
                    return Err("provider event exceeds 1 MiB");
                }
                self.line.push(byte);
                continue;
            }
            self.skip_lf = byte == b'\r';
            let line = std::str::from_utf8(&self.line).map_err(|_| "invalid provider UTF-8")?;
            let line = if std::mem::replace(&mut self.started, true) {
                line
            } else {
                line.trim_start_matches('\u{feff}')
            };
            if line.is_empty() && self.has_data {
                self.data.pop();
                frames.push(std::mem::take(&mut self.data));
                self.has_data = false;
            } else if line == "data" || line.starts_with("data:") {
                let value = line.strip_prefix("data:").unwrap_or("");
                self.data.push_str(value.strip_prefix(' ').unwrap_or(value));
                self.data.push('\n');
                self.has_data = true;
            }
            self.line.clear();
        }
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_handles_fragmented_utf8_crlf_and_rejects_unbounded_events() -> Result<(), &'static str> {
        let wire =
            "\u{feff}: heartbeat\r\nevent: delta\r\ndata: hé\r\ndata: 世界\r\n\r\ndata: [DONE]\n\n";
        for chunk_size in [1, 7, wire.len()] {
            let mut decoder = Decoder::default();
            let mut frames = Vec::new();
            for chunk in wire.as_bytes().chunks(chunk_size) {
                frames.extend(decoder.push(chunk)?);
            }
            assert_eq!(frames, ["hé\n世界", "[DONE]"]);
        }
        assert!(Decoder::default().push(&[0xff, b'\n']).is_err());
        assert!(Decoder::default().push(&vec![b'x'; 1_048_577]).is_err());
        let mut decoder = Decoder::default();
        assert!(decoder.push(b"data: unfinished")?.is_empty());
        Ok(())
    }
}

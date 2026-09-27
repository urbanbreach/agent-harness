use harness_core::tool::ToolError;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

pub(super) const LIMIT: usize = 4 * 1024 * 1024;
pub(super) fn failure(message: &str) -> ToolError {
    ToolError::Execution(message.into())
}

pub(super) struct Wire<R, W> {
    read: BufReader<R>,
    write: W,
}
impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Wire<R, W> {
    pub fn new(read: R, write: W) -> Self {
        Self {
            read: BufReader::new(read),
            write,
        }
    }
    pub async fn send(&mut self, value: Value) -> Result<(), ToolError> {
        let bytes = serde_json::to_vec(&value).map_err(|_| failure("invalid LSP request"))?;
        if bytes.len() > LIMIT {
            return Err(failure("LSP message exceeds 4 MiB"));
        }
        self.write
            .write_all(format!("Content-Length: {}\r\n\r\n", bytes.len()).as_bytes())
            .await?;
        self.write.write_all(&bytes).await?;
        self.write.flush().await?;
        Ok(())
    }
    pub async fn read(&mut self) -> Result<Value, ToolError> {
        let (mut length, mut header_bytes) = (None, 0);
        loop {
            let mut line = Vec::new();
            (&mut self.read)
                .take(8193 - header_bytes)
                .read_until(b'\n', &mut line)
                .await?;
            header_bytes += line.len() as u64;
            if header_bytes > 8192 || !line.is_ascii() || !line.ends_with(b"\r\n") {
                return Err(failure("invalid or oversized LSP header"));
            }
            if line == b"\r\n" {
                break;
            }
            let line = std::str::from_utf8(&line).map_err(|_| failure("invalid LSP header"))?;
            let (name, value) = line
                .trim_end()
                .split_once(':')
                .ok_or_else(|| failure("invalid LSP header"))?;
            if name.eq_ignore_ascii_case("content-length") {
                if length.is_some() {
                    return Err(failure("duplicate LSP content length"));
                }
                length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|_| failure("invalid LSP content length"))?,
                );
            }
            if name.eq_ignore_ascii_case("content-type")
                && value
                    .split(';')
                    .skip(1)
                    .filter_map(|part| part.trim().split_once('='))
                    .any(|(key, value)| {
                        key.eq_ignore_ascii_case("charset")
                            && !["utf-8", "utf8"]
                                .contains(&value.trim_matches('"').to_ascii_lowercase().as_str())
                    })
            {
                return Err(failure("unsupported LSP encoding"));
            }
        }
        let length = length
            .filter(|n| *n <= LIMIT)
            .ok_or_else(|| failure("missing or oversized LSP content length"))?;
        let mut bytes = vec![0; length];
        self.read.read_exact(&mut bytes).await?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| failure("invalid LSP JSON"))?;
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(failure("unsupported LSP JSON-RPC version"));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn framing_handles_fragmentation_and_rejects_ambiguous_or_unbounded_messages(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let value = serde_json::json!({"jsonrpc":"2.0","id":1,"result":"\u{1f600}"});
        let body = value.to_string();
        let frame = format!("Content-Length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{body}", body.len()).repeat(2);
        let (client, mut server) = tokio::io::duplex(32);
        let mut wire = Wire::new(client, tokio::io::sink());
        let (written, read) = tokio::join!(
            async {
                for chunk in frame.as_bytes().chunks(3) {
                    server.write_all(chunk).await?;
                }
                Ok::<_, std::io::Error>(())
            },
            async {
                for _ in 0..2 {
                    assert_eq!(wire.read().await?, value);
                }
                Ok::<_, ToolError>(())
            }
        );
        written?;
        read?;
        for bytes in [
            b"Content-Length: 1\r\nContent-Length: 2\r\n\r\n{}".to_vec(),
            b"Content-Length: 4194305\r\n\r\n".to_vec(),
            b"Content-Type: application/json; charset=utf-16\r\nContent-Length: 2\r\n\r\n{}"
                .to_vec(),
            b"Content-Length: 2\r\n\r\n{".to_vec(),
            vec![b'a'; 8193],
        ] {
            assert!(Wire::new(bytes.as_slice(), tokio::io::sink())
                .read()
                .await
                .is_err());
        }
        Ok(())
    }
}

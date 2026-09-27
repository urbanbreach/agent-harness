use super::AcpTransport;

#[derive(Default)]
pub struct MockAcpTransport {
    pub connected: bool,
    pub fail_connect: bool,
    pub fail_connect_reason: String,
    pub disconnect_on_next_operate: bool,
    pub fail_on_next_operate: bool,
    pub fail_operate_reason: String,
    pub operate_calls: usize,
    pub last_payload: Option<Vec<u8>>,
}
impl MockAcpTransport {
    pub fn new() -> Self {
        Self {
            fail_connect_reason: "mock connect failed".into(),
            fail_operate_reason: "mock operate failed".into(),
            ..Self::default()
        }
    }
}
impl AcpTransport for MockAcpTransport {
    fn connect(&mut self) -> Result<(), String> {
        self.connected = !self.fail_connect;
        if self.connected {
            Ok(())
        } else {
            Err(self.fail_connect_reason.clone())
        }
    }
    fn disconnect(&mut self) -> Result<(), String> {
        self.connected = false;
        Ok(())
    }
    fn operate(&mut self, payload: &[u8]) -> Result<Vec<u8>, String> {
        if !self.connected {
            return Err("not connected".into());
        }
        self.operate_calls = self.operate_calls.saturating_add(1);
        if std::mem::take(&mut self.disconnect_on_next_operate) {
            self.connected = false;
            return Err("peer disconnected".into());
        }
        if std::mem::take(&mut self.fail_on_next_operate) {
            return Err(self.fail_operate_reason.clone());
        }
        if payload.len() > 1024 * 1024 {
            return Err("payload exceeds 1 MiB".into());
        }
        self.last_payload = Some(payload.into());
        Ok(payload.into())
    }
}

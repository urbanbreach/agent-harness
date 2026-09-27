use super::{failure, LIMIT};
use futures_util::{SinkExt, StreamExt};
use harness_core::{
    process::{spawn_group, Group},
    tool::ToolError,
};
use rmcp::{
    model::{ClientJsonRpcMessage, ServerJsonRpcMessage},
    transport::{async_rw::JsonRpcMessageCodec, Transport},
    RoleClient,
};
use std::{process::Stdio, sync::Arc};
use tokio::{
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};
use tokio_util::codec::{FramedRead, FramedWrite};

pub(super) struct Process {
    child: Child,
    group: Group,
    read: FramedRead<ChildStdout, JsonRpcMessageCodec<ServerJsonRpcMessage>>,
    write: Arc<Mutex<FramedWrite<ChildStdin, JsonRpcMessageCodec<ClientJsonRpcMessage>>>>,
}
impl Process {
    pub fn start(mut command: Command) -> Result<Self, ToolError> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (mut child, group) = spawn_group(command)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| failure("MCP stdout is unavailable"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| failure("MCP stdin is unavailable"))?;
        Ok(Self {
            child,
            group,
            read: FramedRead::new(stdout, JsonRpcMessageCodec::new_with_max_length(LIMIT)),
            write: Arc::new(Mutex::new(FramedWrite::new(
                stdin,
                JsonRpcMessageCodec::new_with_max_length(LIMIT),
            ))),
        })
    }
}
impl Transport<RoleClient> for Process {
    type Error = std::io::Error;
    fn send(
        &mut self,
        message: ClientJsonRpcMessage,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send + 'static {
        let write = Arc::clone(&self.write);
        async move {
            write
                .lock()
                .await
                .send(message)
                .await
                .map_err(std::io::Error::other)
        }
    }
    async fn receive(&mut self) -> Option<ServerJsonRpcMessage> {
        self.read.next().await?.ok()
    }
    async fn close(&mut self) -> Result<(), Self::Error> {
        self.group.terminate(&mut self.child).await.map(|_| ())
    }
}

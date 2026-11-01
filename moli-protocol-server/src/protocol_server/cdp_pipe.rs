use std::{
    io::{self, Read, Write},
    os::fd::FromRawFd,
    pin::Pin,
    task::{Context as TaskContext, Poll, ready},
};

use anyhow::{Context, Result};
use axum::extract::ws::Message;
use moli_protocol::ParsedCdpCommand;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf};

use super::{AppState, BrowserInstanceControl, InstanceCommand, ProtocolServer};
use crate::{cdp_frontend::CdpFrontendEndpoint, cdp_writer::spawn_socket_sink};

struct PipeFd(tokio::io::unix::AsyncFd<std::fs::File>);

impl PipeFd {
    fn new(file: std::fs::File) -> io::Result<Self> {
        socket2::SockRef::from(&file).set_nonblocking(true)?;
        Ok(Self(tokio::io::unix::AsyncFd::new(file)?))
    }
}

impl AsyncRead for PipeFd {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            let mut guard = ready!(self.0.poll_read_ready(cx))?;
            match guard.try_io(|fd| fd.get_ref().read(buf.initialize_unfilled())) {
                Ok(Ok(length)) => {
                    buf.advance(length);
                    return Poll::Ready(Ok(()));
                }
                Ok(Err(error)) => return Poll::Ready(Err(error)),
                Err(_) => continue,
            }
        }
    }
}

impl AsyncWrite for PipeFd {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            let mut guard = ready!(self.0.poll_write_ready(cx))?;
            match guard.try_io(|fd| fd.get_ref().write(buf)) {
                Ok(result) => return Poll::Ready(result),
                Err(_) => continue,
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl ProtocolServer {
    /// Serves one browser-level CDP session over the Chromium pipe protocol:
    /// NUL-terminated JSON messages are read from inherited descriptor 3 and
    /// written to inherited descriptor 4. No socket is bound.
    pub async fn serve_pipe(&self) -> Result<()> {
        // SAFETY: the pipe protocol hands this process descriptors 3 and 4. They
        // are taken exactly once, here, and owned by the returned files.
        let input = unsafe { std::fs::File::from_raw_fd(3) };
        let output = unsafe { std::fs::File::from_raw_fd(4) };
        self.serve_pipe_io(PipeFd::new(input)?, PipeFd::new(output)?)
            .await
    }

    async fn serve_pipe_io(
        &self,
        input: impl AsyncRead + Unpin,
        output: impl AsyncWrite + Unpin + Send + 'static,
    ) -> Result<()> {
        let control = BrowserInstanceControl::new();
        let state = AppState::new_with_storage_partition_and_runtime_config(
            ([127, 0, 0, 1], 0).into(),
            self.storage_partition.clone(),
            self.navigation_runtime_config.clone(),
            self.config.screencast_interval_ms,
            control.clone(),
        )?;
        let endpoint = state.cdp_owner_registry.shared_owner()?;
        let result = run_pipe(input, output, endpoint, control.clone()).await;
        control.request(InstanceCommand::Shutdown);
        state.cdp_owner_registry.shutdown().await;
        result
    }
}

async fn run_pipe(
    input: impl AsyncRead + Unpin,
    output: impl AsyncWrite + Unpin + Send + 'static,
    endpoint: CdpFrontendEndpoint,
    control: BrowserInstanceControl,
) -> Result<()> {
    let writer = futures_util::sink::unfold(output, |mut output, message| async move {
        if let Message::Text(text) = message {
            output.write_all(text.as_bytes()).await?;
            output.write_all(&[0]).await?;
            output.flush().await?;
        }
        Ok::<_, std::io::Error>(output)
    });
    let (sink, mut finished) = spawn_socket_sink(Box::pin(writer));
    let frontend_id = endpoint.attach_browser(sink.clone()).await?;
    let mut reader = BufReader::new(input);
    let result = async {
        let mut frame = Vec::new();
        loop {
            frame.clear();
            let count = tokio::select! {
                _ = control.shutdown_requested() => break,
                _ = endpoint.wait_for_shutdown() => break,
                _ = &mut finished => break,
                result = reader.read_until(0, &mut frame) => result.context("CDP pipe read failed")?,
            };
            if count == 0 {
                break;
            }
            anyhow::ensure!(frame.pop() == Some(0), "unterminated CDP pipe message");
            let text = String::from_utf8(frame.clone()).context("CDP pipe message is not UTF-8")?;
            if !endpoint.command(frontend_id, ParsedCdpCommand::parse_str(text)).await {
                break;
            }
        }
        Ok(())
    }.await;
    endpoint.detach_browser(frontend_id);
    sink.close();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServerConfig;
    use serde_json::Value;
    use tokio::io::AsyncReadExt;

    async fn exercise_pipe(close_command: bool) {
        let (client_input, server_input) = std::os::unix::net::UnixStream::pair().unwrap();
        let (server_output, client_output) = std::os::unix::net::UnixStream::pair().unwrap();
        let server_input = PipeFd::new(std::fs::File::from(std::os::fd::OwnedFd::from(
            server_input,
        )))
        .unwrap();
        let server_output = PipeFd::new(std::fs::File::from(std::os::fd::OwnedFd::from(
            server_output,
        )))
        .unwrap();
        client_input.set_nonblocking(true).unwrap();
        client_output.set_nonblocking(true).unwrap();
        let client_input = tokio::net::UnixStream::from_std(client_input).unwrap();
        let client_output = tokio::net::UnixStream::from_std(client_output).unwrap();
        let server = ProtocolServer::new(ServerConfig::default());
        let serving =
            tokio::spawn(async move { server.serve_pipe_io(server_input, server_output).await });
        let mut input = client_input;
        let mut output = BufReader::new(client_output);
        input
            .write_all(b"{\"id\":1,\"method\":\"Browser.")
            .await
            .unwrap();
        input.write_all(b"getVersion\"}\0{\"id\":2,\"method\":\"Target.createTarget\",\"params\":{\"url\":\"about:blank\"}}\0").await.unwrap();
        let mut replies = Vec::new();
        while replies.len() < 2 {
            let mut frame = Vec::new();
            output.read_until(0, &mut frame).await.unwrap();
            assert_eq!(frame.pop(), Some(0));
            let response: Value = serde_json::from_slice(&frame).unwrap();
            if response.get("id").is_some() {
                replies.push(response);
            }
        }
        assert!(
            replies
                .iter()
                .any(|r| r["id"] == 1 && r["result"]["product"].is_string())
        );
        assert!(
            replies
                .iter()
                .any(|r| r["id"] == 2 && r["result"]["targetId"].is_string())
        );
        if close_command {
            input
                .write_all(b"{\"id\":3,\"method\":\"Browser.close\"}\0")
                .await
                .unwrap();
        } else {
            input.shutdown().await.unwrap();
        }
        tokio::time::timeout(std::time::Duration::from_secs(5), serving)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let mut tail = Vec::new();
        output.read_to_end(&mut tail).await.unwrap();
    }
    #[tokio::test]
    async fn pipe_routes_fragmented_commands_and_stops_on_eof() {
        exercise_pipe(false).await;
    }

    #[tokio::test]
    async fn pipe_browser_close_stops_with_input_still_open() {
        exercise_pipe(true).await;
    }
}

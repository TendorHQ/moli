use super::*;
use moli_cookie_jar::new_shared_browser_cookie_store;
use std::{io::Write, sync::mpsc as std_mpsc};

fn submit_html_request(
    config: &FetchConfig,
    request: Request,
) -> (
    FetchRuntimeOwner,
    FetchCancelHandle,
    PendingStreamingHtmlResponse,
) {
    let owner = FetchRuntimeOwner::new(config, new_shared_browser_cookie_store());
    let pending = owner.handle().submit_html_stream(request).unwrap();
    let cancel = pending.cancel_handle.clone();
    (owner, cancel, pending)
}

#[tokio::test]
async fn html_redirect_timing_belongs_to_the_final_transfer() -> Result<()> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/redirect", listener.local_addr()?);
    let (release_redirect, ready) = std_mpsc::sync_channel(1);
    let server = thread::spawn(move || -> std::io::Result<()> {
        for path in ["/redirect", "/final"] {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte)?;
                request.extend_from_slice(&byte);
            }
            assert!(request.starts_with(format!("GET {path} ").as_bytes()));
            if path == "/redirect" {
                stream.write_all(
                    b"HTTP/1.1 103 Early Hints\r\n\r\n\
                      HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 8\r\nConnection: close\r\n\r\nold!",
                )?;
                ready.recv_timeout(Duration::from_secs(5)).unwrap();
                stream.write_all(b"tail")?;
            } else {
                stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )?;
            }
        }
        Ok(())
    });
    let (_owner, cancel, pending) =
        submit_html_request(&FetchConfig::default(), Request::get(&url)?);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cancel.response_timing().body_bytes_received() {
        anyhow::ensure!(Instant::now() < deadline, "redirect body was not observed");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let redirect_timing = cancel.response_timing();
    assert!(
        redirect_timing
            .first_interim_response_started_at()
            .is_some()
    );
    assert!(redirect_timing.response_started_at().is_some());
    let redirect_finished_after = Instant::now();
    release_redirect.send(())?;

    let mut response = pending.into_response().await?;
    assert_eq!(response.final_url.path(), "/final");
    assert!(response.next_chunk().await.is_none());
    response.finish().await?;
    let timing = cancel.response_timing();
    assert!(timing.request_started_at().unwrap() >= redirect_finished_after);
    assert!(timing.response_started_at().unwrap() >= redirect_finished_after);
    assert!(timing.first_interim_response_started_at().is_none());
    assert!(!timing.body_bytes_received());
    server.join().expect("server thread")?;
    Ok(())
}

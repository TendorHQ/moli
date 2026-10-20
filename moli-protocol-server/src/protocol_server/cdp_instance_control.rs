//! Translate CDP instance commands without entering a page's work queue.

use std::time::Duration;

use moli_protocol::ParsedCdpCommand;
use serde_json::{Value, json};

use super::{BrowserInstanceControl, InstanceCommand};
use crate::{cdp_frontend::CdpFrontendEndpoint, cdp_writer::CdpSocketSink};

const BROWSER_CLOSE_FLUSH_BUDGET: Duration = Duration::from_secs(1);

/// Return true when this adapter consumed a command and the frontend should
/// close, including when its acknowledgment could not be flushed.
pub(super) async fn handle_instance_command(
    command: &ParsedCdpCommand,
    endpoint: &CdpFrontendEndpoint,
    frontend_id: u64,
    sink: &CdpSocketSink,
    control: &BrowserInstanceControl,
) -> bool {
    let Some((command, response)) = instance_command(command, endpoint, frontend_id) else {
        return false;
    };
    acknowledge_instance_command(sink, control, command, response).await;
    true
}

// Instance control bypasses page work, using the normal wire parser and the
// owner's session registry. Chromium permits it on page and attached routes too.
fn instance_command(
    command: &ParsedCdpCommand,
    endpoint: &CdpFrontendEndpoint,
    frontend_id: u64,
) -> Option<(InstanceCommand, Value)> {
    let request = command.request();
    if request.method() != "Browser.close"
        || !endpoint.owns_session(frontend_id, request.session_id())
    {
        return None;
    }
    let mut response = json!({"id": request.id(), "result": {}});
    if let Some(session_id) = request.session_id() {
        response["sessionId"] = json!(session_id);
    }
    Some((InstanceCommand::Shutdown, response))
}

async fn acknowledge_instance_command(
    sink: &CdpSocketSink,
    control: &BrowserInstanceControl,
    command: InstanceCommand,
    response: Value,
) -> bool {
    if !sink.enqueue_owned_message(response) {
        return false;
    }
    if !matches!(
        tokio::time::timeout(BROWSER_CLOSE_FLUSH_BUDGET, sink.flush()).await,
        Ok(true)
    ) {
        return false;
    }
    control.request(command);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_valid_owned_frontend_close_can_request_instance_shutdown() {
        let (endpoint, _receivers, router) = crate::cdp_frontend::cdp_frontend_channel();
        router
            .register_browser_frontend(1, "hidden-browser".into(), CdpSocketSink::for_test())
            .unwrap();
        router
            .register_target_frontend(
                2,
                "page".into(),
                "hidden-page".into(),
                CdpSocketSink::for_test(),
            )
            .unwrap();
        for value in [
            json!({"id": 7, "method": "Browser.close"}),
            json!({"id": 7, "method": "Browser.close", "params": {}}),
            json!({"id": 7, "method": "Browser.close", "params": {"ignored": true}}),
            json!({"id": 7, "method": "Browser.close", "params": []}),
            json!({"id": 7, "method": "Browser.close", "params": null}),
        ] {
            let command = ParsedCdpCommand::parse_value(value).unwrap();
            for frontend_id in [1, 2] {
                assert_eq!(
                    instance_command(&command, &endpoint, frontend_id),
                    Some((InstanceCommand::Shutdown, json!({"id": 7, "result": {}})))
                );
            }
        }
        for value in [
            json!({"id": 7, "method": "Browser.close", "sessionId": "unknown"}),
            json!({"id": 7, "method": "Browser.close", "sessionId": "hidden-browser"}),
            json!({"id": 7, "method": "browser.close"}),
        ] {
            let command = ParsedCdpCommand::parse_value(value).unwrap();
            assert!(instance_command(&command, &endpoint, 1).is_none());
        }
        for value in [
            json!({"id": 7, "method": "Browser.close", "sessionId": null}),
            json!({"id": -1, "method": "Browser.close"}),
            json!({"id": "7", "method": "Browser.close"}),
            json!({"method": "Browser.close"}),
        ] {
            assert!(ParsedCdpCommand::parse_value(value).is_err());
        }
        drop(router);
        let command =
            ParsedCdpCommand::parse_value(json!({"id": 7, "method": "Browser.close"})).unwrap();
        assert!(
            instance_command(&command, &endpoint, 1).is_none(),
            "a transport must not keep its departed owner alive"
        );
    }

    #[tokio::test]
    async fn failed_or_unconfirmed_flush_does_not_stop_the_instance() {
        let control = BrowserInstanceControl::new();
        let sink = CdpSocketSink::for_test();
        assert!(
            !acknowledge_instance_command(
                &sink,
                &control,
                InstanceCommand::Shutdown,
                json!({"id": 1, "result": {}})
            )
            .await
        );
        let (sink, _stalled_writer) = CdpSocketSink::with_stalled_writer_for_test(2);
        assert!(
            !acknowledge_instance_command(
                &sink,
                &control,
                InstanceCommand::Shutdown,
                json!({"id": 2, "result": {}})
            )
            .await
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(10), control.shutdown_requested())
                .await
                .is_err()
        );
        assert!(control.connection().is_some());
    }
}

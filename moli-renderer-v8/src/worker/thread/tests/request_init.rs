use super::*;

#[tokio::test]
async fn worker_request_and_fetch_use_the_shared_dictionary_conversion() {
    ensure_v8();
    let source = format!(
        "({}).then(value=>postMessage(value),error=>postMessage(String(error.stack||error)));",
        include_str!("../../../../tests/fixtures/request-init-dictionary.js")
    );
    let mut handle = spawn_worker(source, "https://request-init-worker.test/worker.js".into());
    let message = timeout(TIMEOUT, handle.recv())
        .await
        .expect("worker dictionary conversion must settle")
        .expect("worker must report its result");
    assert_eq!(expect_post_json(message), "true");
}

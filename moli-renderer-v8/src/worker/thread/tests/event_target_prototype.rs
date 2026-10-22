use super::*;

#[tokio::test]
async fn worker_event_target_prototype_methods_preserve_native_state() {
    ensure_v8();
    let probe = include_str!("../../../../tests/fixtures/event-target-prototype.js");
    let mut handle = spawn_worker(
        format!("{probe}\npostMessage(__uiEventResults); close();"),
        "https://event-target-prototype.test/worker.js".into(),
    );
    let message = timeout(TIMEOUT, handle.recv()).await.unwrap().unwrap();
    let result: serde_json::Value = serde_json::from_str(&expect_post_json(message)).unwrap();
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["total"], 56, "{result}");
    assert_eq!(result["passed"], result["total"], "{result}");
}

use super::*;
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type FontGates = HashMap<String, tokio::sync::oneshot::Sender<()>>;

async fn gated_fonts(
    names: &[&str],
) -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<String>,
    FontGates,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (requests_tx, requests_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut gates = HashMap::new();
    let mut releases = HashMap::new();
    for name in names {
        let (tx, rx) = tokio::sync::oneshot::channel();
        gates.insert((*name).to_owned(), tx);
        releases.insert((*name).to_owned(), rx);
    }
    let server = tokio::spawn(async move {
        let mut responses = Vec::new();
        while !releases.is_empty() {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&buffer[..read]);
            }
            let request = String::from_utf8(request).unwrap();
            let path = request.split_whitespace().nth(1).unwrap().to_owned();
            let release = releases.remove(path.rsplit('/').next().unwrap()).unwrap();
            requests_tx.send(path).unwrap();
            responses.push(tokio::spawn(async move {
                let _ = release.await;
                let font = include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../moli-layout/tests/fixtures/moli-ahem.woff2"
                ));
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: font/woff2\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    font.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(font).await;
            }));
        }
        for response in responses {
            response.await.unwrap();
        }
    });
    (origin, requests_rx, gates, server)
}

async fn assert_mixed_ready(js_starts_first: bool, js_finishes_first: bool) {
    let (origin, mut requests, mut gates, server) = gated_fonts(&["css.woff2", "js.woff2"]).await;
    let loader = ResourceRequestClient::new(&moli_fetch::FetchConfig::default()).unwrap();
    loader.set_optional_resource_fetch_mask(crate::protocol_types::OptionalResourceFetchMask::FONT);
    let (mut vm, mut completions) = new_parsed_test_vm_with_loader_and_resource_completion_queue(
        &format!("{origin}/page"),
        "<!doctype html><html><head></head><body>AAAA</body></html>",
        &loader,
    );
    vm.set_layout_policy(moli_page_types::LayoutPolicy::OnDemand);
    vm.eval("globalThis.fonts=document.fonts;globalThis.readyResults=[];globalThis.doneEvents=[];fonts.addEventListener('loadingdone',event=>doneEvents.push(event.fontfaces.map(face=>face.family).sort().join(',')))").unwrap();
    let start_css = format!(
        r#"const style=document.createElement('style');
        style.textContent='@font-face{{font-family:AuditCSS;src:url("{origin}/css.woff2")}}';
        document.head.append(style);document.body.style.fontFamily='AuditCSS';
        globalThis.cssReady=fonts.ready;
        globalThis.cssFace=[...fonts].find(face=>face.family==='AuditCSS');
        cssReady.then(value=>readyResults.push('css:'+String(value===fonts)));"#
    );
    let start_js = format!(
        r#"globalThis.jsFace=new FontFace('AuditJS','url("{origin}/js.woff2")');
        fonts.add(jsFace);jsFace.load();globalThis.jsReady=fonts.ready;
        jsReady.then(value=>readyResults.push('js:'+String(value===fonts)));"#
    );
    for (program, expected) in if js_starts_first {
        [(&start_js, "/js.woff2"), (&start_css, "/css.woff2")]
    } else {
        [(&start_css, "/css.woff2"), (&start_js, "/js.woff2")]
    } {
        vm.eval(program).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), requests.recv())
                .await
                .unwrap()
                .unwrap(),
            expected
        );
    }
    assert_eq!(
        vm.eval("JSON.stringify([cssFace.status,jsFace.status,cssReady===jsReady,readyResults,doneEvents])").unwrap(),
        r#"["loading","loading",true,[],[]]"#
    );
    let order = if js_finishes_first {
        ["js.woff2", "css.woff2"]
    } else {
        ["css.woff2", "js.woff2"]
    };
    for (index, name) in order.into_iter().enumerate() {
        gates.remove(name).unwrap().send(()).unwrap();
        assert!(
            tokio::time::timeout(
                Duration::from_secs(2),
                completions.wait_for_arrival_without_timeout()
            )
            .await
            .unwrap()
        );
        let completion = completions.pop_next_async_subresource_event().unwrap();
        let activity = vm
            .complete_async_subresource_fetch_event_body(completion)
            .unwrap();
        vm.finish_async_subresource_body_checkpoint_for_test(activity)
            .unwrap();
        let expected = if index == 1 {
            r#"["loaded","loaded",["css:true","js:true"],["AuditCSS,AuditJS"]]"#
        } else if js_finishes_first {
            r#"["loading","loaded",[],[]]"#
        } else {
            r#"["loaded","loading",[],[]]"#
        };
        // Read only captured promises/results here: another fonts.ready or
        // fonts.status getter must not be needed to finish the loading cycle.
        assert_eq!(
            vm.eval("JSON.stringify([cssFace.status,jsFace.status,readyResults.slice().sort(),doneEvents])").unwrap(),
            expected
        );
    }
    server.await.unwrap();
}

#[tokio::test]
async fn font_set_ready_waits_for_js_after_css_started_and_finished_first() {
    assert_mixed_ready(false, false).await;
}

#[tokio::test]
async fn font_set_ready_preserves_js_promise_when_css_finishes_first() {
    assert_mixed_ready(true, false).await;
}

#[tokio::test]
async fn font_set_ready_waits_for_css_after_js_finishes_first() {
    assert_mixed_ready(false, true).await;
}

#[tokio::test]
async fn font_set_ready_preserves_js_promise_when_js_finishes_first() {
    assert_mixed_ready(true, true).await;
}

#[tokio::test]
async fn font_set_ready_settles_css_started_by_explicit_font_face_load() {
    let (origin, mut requests, mut gates, server) = gated_fonts(&["css.woff2"]).await;
    let loader = ResourceRequestClient::new(&moli_fetch::FetchConfig::default()).unwrap();
    loader.set_optional_resource_fetch_mask(crate::protocol_types::OptionalResourceFetchMask::FONT);
    let (mut vm, mut completions) = new_parsed_test_vm_with_loader_and_resource_completion_queue(
        &format!("{origin}/page"),
        "<!doctype html><html><head></head><body>AAAA</body></html>",
        &loader,
    );
    vm.set_layout_policy(moli_page_types::LayoutPolicy::OnDemand);
    vm.eval(&format!(
        r#"
        const style=document.createElement('style');
        style.textContent='@font-face{{font-family:AuditCSS;src:url("{origin}/css.woff2")}}';
        document.head.append(style);
        globalThis.fonts=document.fonts;globalThis.face=[...fonts][0];
        face.load();globalThis.ready=fonts.ready;globalThis.readyResolved=false;
        ready.then(value=>readyResolved=value===fonts);
    "#
    ))
    .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), requests.recv())
            .await
            .unwrap()
            .unwrap(),
        "/css.woff2"
    );
    gates.remove("css.woff2").unwrap().send(()).unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_secs(2),
            completions.wait_for_arrival_without_timeout()
        )
        .await
        .unwrap()
    );
    let completion = completions.pop_next_async_subresource_event().unwrap();
    let activity = vm
        .complete_async_subresource_fetch_event_body(completion)
        .unwrap();
    vm.finish_async_subresource_body_checkpoint_for_test(activity)
        .unwrap();
    assert_eq!(
        vm.eval("JSON.stringify([face.status,readyResolved])")
            .unwrap(),
        r#"["loaded",true]"#
    );
    server.await.unwrap();
}

#[tokio::test]
async fn font_face_url_is_frozen_before_base_href_changes() {
    let (origin, mut requests, mut gates, server) = gated_fonts(&["font.woff2"]).await;
    let loader = ResourceRequestClient::new(&moli_fetch::FetchConfig::default()).unwrap();
    let (mut vm, mut completions) = new_parsed_test_vm_with_loader_and_resource_completion_queue(
        &format!("{origin}/page"),
        "<!doctype html><html><head><base href='/before/'></head><body></body></html>",
        &loader,
    );
    vm.eval(
        r#"
        globalThis.face=new FontFace('Relative','url(font.woff2)');
        document.querySelector('base').href='/after/';
        globalThis.loaded=false;face.load().then(value=>loaded=value===face);
    "#,
    )
    .unwrap();
    let request = tokio::time::timeout(Duration::from_secs(2), requests.recv())
        .await
        .unwrap()
        .unwrap();
    gates.remove("font.woff2").unwrap().send(()).unwrap();
    server.await.unwrap();
    assert_eq!(request, "/before/font.woff2");
    assert!(
        tokio::time::timeout(
            Duration::from_secs(2),
            completions.wait_for_arrival_without_timeout()
        )
        .await
        .unwrap()
    );
    let completion = completions.pop_next_async_subresource_event().unwrap();
    let activity = vm
        .complete_async_subresource_fetch_event_body(completion)
        .unwrap();
    vm.finish_async_subresource_body_checkpoint_for_test(activity)
        .unwrap();
    assert_eq!(
        vm.eval("JSON.stringify([face.status,loaded])").unwrap(),
        r#"["loaded",true]"#
    );
}

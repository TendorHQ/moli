use super::*;

pub(super) fn order_font_data() -> (String, String) {
    let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    (
        encode(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../moli-layout/tests/fixtures/moli-ahem-narrow.ttf"
        ))),
        encode(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../moli-layout/tests/fixtures/moli-ahem-wide.ttf"
        ))),
    )
}

pub(super) fn order_vm() -> StandaloneScriptVmHarness {
    let mut vm = new_parsed_test_vm(
        "https://font-selection-order.test/",
        "<!doctype html><html><head></head><body><span id='probe' style='display:inline-block;font:40px OrderFace'>AAAA</span><iframe id='frame'></iframe></body></html>",
    );
    vm.set_layout_policy(moli_page_types::LayoutPolicy::OnDemand);
    vm
}

#[tokio::test]
async fn font_face_css_selection_keeps_declaration_order_without_js_projection() {
    use crate::native_bridge::document::take_owner_font_face_projection_count_for_test;

    let (narrow, wide) = order_font_data();
    for (sources, expected) in [([&narrow, &wide], "160"), ([&wide, &narrow], "80")] {
        let loader = ResourceRequestClient::new(&moli_fetch::FetchConfig::default()).unwrap();
        loader.set_optional_resource_fetch_mask(
            crate::protocol_types::OptionalResourceFetchMask::FONT,
        );
        let (mut vm, _completions) = new_parsed_test_vm_with_loader_and_resource_completion_queue(
            "https://font-selection-order.test/",
            "<!doctype html><html><head></head><body><span id='probe' style='display:inline-block;font:40px OrderFace'>AAAA</span></body></html>",
            &loader,
        );
        vm.set_layout_policy(moli_page_types::LayoutPolicy::OnDemand);
        take_owner_font_face_projection_count_for_test();
        vm.eval(&format!(r#"
            Object.defineProperty(document, 'fonts', {{get() {{throw new Error('unexpected document.fonts access')}}}});
            const style=document.createElement('style');
            style.textContent='@font-face{{font-family:OrderFace;src:url(data:font/ttf;base64,{})}}@font-face{{font-family:OrderFace;src:url(data:font/ttf;base64,{})}}';
            document.head.append(style);
        "#, sources[0], sources[1])).unwrap();
        vm.reconcile_document_web_fonts_for_layout();
        assert_eq!(vm.document_web_font_counts_for_test(), (2, 2, 2));
        assert_eq!(
            vm.eval("String(probe.getBoundingClientRect().width)")
                .unwrap(),
            expected
        );
        assert_eq!(take_owner_font_face_projection_count_for_test(), 0);
    }
}

#[tokio::test]
async fn font_face_css_reordering_preserves_last_duplicate_and_manual_priority() {
    use crate::native_bridge::document::take_owner_font_face_projection_count_for_test;

    let loader = ResourceRequestClient::new(&moli_fetch::FetchConfig::default()).unwrap();
    loader.set_optional_resource_fetch_mask(crate::protocol_types::OptionalResourceFetchMask::FONT);
    let (mut vm, _completions) = new_parsed_test_vm_with_loader_and_resource_completion_queue(
        "https://font-selection-order.test/",
        "<!doctype html><html><head></head><body><span id='probe' style='display:inline-block;font:40px OrderFace'>AAAA</span></body></html>",
        &loader,
    );
    vm.set_layout_policy(moli_page_types::LayoutPolicy::OnDemand);
    let (narrow, wide) = order_font_data();
    take_owner_font_face_projection_count_for_test();
    vm.eval(&format!(r#"
        globalThis.styles=["{narrow}","{wide}"].map(data=>{{
            const style=document.createElement('style');
            style.textContent=`@font-face{{font-family:OrderFace;src:url(data:font/ttf;base64,${{data}})}}`;
            document.head.append(style);return style;
        }});
    "#)).unwrap();
    let width = |vm: &mut StandaloneScriptVmHarness| {
        vm.reconcile_document_web_fonts_for_layout();
        assert_eq!(vm.document_web_font_counts_for_test(), (2, 2, 2));
        vm.eval("String(probe.getBoundingClientRect().width)")
            .unwrap()
    };
    assert_eq!(width(&mut vm), "160");
    vm.eval("document.head.append(styles[0])").unwrap();
    assert_eq!(width(&mut vm), "80");
    vm.eval("globalThis.duplicate=styles[1].cloneNode(true);document.head.append(duplicate)")
        .unwrap();
    assert_eq!(width(&mut vm), "160");
    vm.eval("duplicate.remove()").unwrap();
    assert_eq!(width(&mut vm), "80");
    assert_eq!(take_owner_font_face_projection_count_for_test(), 0);

    vm.eval(&format!(r#"
        document.fonts.add(new FontFace('OrderFace',Uint8Array.from(atob('{wide}'),c=>c.charCodeAt(0))));
        document.head.append(styles[1]);
    "#)).unwrap();
    vm.reconcile_document_web_fonts_for_layout();
    assert_eq!(vm.document_web_font_counts_for_test(), (2, 2, 3));
    vm.eval("document.head.append(styles[0])").unwrap();
    vm.reconcile_document_web_fonts_for_layout();
    assert_eq!(
        vm.eval("String(probe.getBoundingClientRect().width)")
            .unwrap(),
        "160"
    );
    vm.eval("document.fonts.clear()").unwrap();
    assert_eq!(
        vm.eval("String(probe.getBoundingClientRect().width)")
            .unwrap(),
        "80"
    );
}

#[tokio::test]
async fn font_face_css_order_is_published_before_child_font_admission() {
    use crate::native_bridge::document::take_owner_font_face_projection_count_for_test;

    let loader = ResourceRequestClient::new(&moli_fetch::FetchConfig::default()).unwrap();
    loader.set_optional_resource_fetch_mask(crate::protocol_types::OptionalResourceFetchMask::FONT);
    let (mut vm, _completions) = new_parsed_test_vm_with_loader_and_resource_completion_queue(
        "https://font-selection-order.test/",
        "<!doctype html><iframe id='frame'></iframe>",
        &loader,
    );
    vm.set_layout_policy(moli_page_types::LayoutPolicy::OnDemand);
    let (narrow, wide) = order_font_data();
    take_owner_font_face_projection_count_for_test();
    for (first, second, expected) in [(&narrow, &wide, "160"), (&wide, &narrow, "80")] {
        vm.eval(&format!(r#"
            globalThis.child=frame.contentDocument;
            child.open();
            child.write('<!doctype html><style>@font-face{{font-family:OrderFace;src:url(data:font/ttf;base64,{first})}}@font-face{{font-family:OrderFace;src:url(data:font/ttf;base64,{second})}}</style><span id="target" style="display:inline-block;font:40px OrderFace">AAAA</span>');
            child.close();
            Object.defineProperty(child,'fonts',{{configurable:true,get(){{throw new Error('unexpected child.fonts access')}}}});
        "#)).unwrap();
        vm.reconcile_document_web_fonts_for_layout();
        assert_eq!(
            vm.eval("String(child.getElementById('target').getBoundingClientRect().width)")
                .unwrap(),
            expected
        );
        assert_eq!(vm.document_web_font_counts_for_test(), (0, 0, 0));
        assert_eq!(take_owner_font_face_projection_count_for_test(), 0);
    }
}

#[test]
fn font_face_selection_follows_owner_membership_instead_of_resource_id() {
    let mut vm = order_vm();
    let (narrow, wide) = order_font_data();
    vm.eval(&format!(
        r#"
        const bytes=data=>Uint8Array.from(atob(data),c=>c.charCodeAt(0));
        globalThis.narrow=new FontFace('OrderFace',bytes('{narrow}'));
        globalThis.wide=new FontFace('OrderFace',bytes('{wide}'));
        document.fonts.add(narrow);
    "#
    ))
    .unwrap();
    let width = |vm: &mut StandaloneScriptVmHarness| {
        vm.eval("String(probe.getBoundingClientRect().width)")
            .unwrap()
    };
    assert_eq!(width(&mut vm), "80");
    vm.eval("document.fonts.add(wide)").unwrap();
    assert_eq!(width(&mut vm), "160");
    vm.eval("document.fonts.delete(narrow);document.fonts.delete(wide);document.fonts.add(wide);document.fonts.add(narrow)").unwrap();
    assert_eq!(
        vm.eval("[...document.fonts].map(f=>f===wide?'wide':'narrow').join(',')")
            .unwrap(),
        "wide,narrow"
    );
    assert_eq!(width(&mut vm), "80");
    vm.eval("document.fonts.delete(wide);document.fonts.add(wide)")
        .unwrap();
    assert_eq!(width(&mut vm), "160");
}

#[test]
fn manually_added_font_faces_take_priority_over_css_connected_faces() {
    let mut vm = order_vm();
    let (narrow, wide) = order_font_data();
    vm.eval(&format!(r#"
        const style=document.createElement('style');
        style.textContent='@font-face{{font-family:OrderFace;src:url(data:font/ttf;base64,{narrow})}}';
        document.head.append(style);
        globalThis.narrow=[...document.fonts][0];
        narrow.load();
    "#)).unwrap();
    assert_eq!(vm.eval("narrow.status").unwrap(), "loaded");
    assert_eq!(
        vm.eval("String(probe.getBoundingClientRect().width)")
            .unwrap(),
        "80"
    );
    vm.eval(&format!(r#"
        globalThis.wide=new FontFace('OrderFace',Uint8Array.from(atob('{wide}'),c=>c.charCodeAt(0)));
        document.fonts.add(wide);
    "#)).unwrap();
    assert_eq!(
        vm.eval("String(probe.getBoundingClientRect().width)")
            .unwrap(),
        "160"
    );
    vm.eval("document.fonts.delete(wide)").unwrap();
    assert_eq!(
        vm.eval("String(probe.getBoundingClientRect().width)")
            .unwrap(),
        "80"
    );
}

#[test]
fn shared_font_faces_have_independent_selection_order_in_each_document() {
    let mut vm = order_vm();
    let (narrow, wide) = order_font_data();
    vm.eval(&format!(r#"
        const bytes=data=>Uint8Array.from(atob(data),c=>c.charCodeAt(0));
        globalThis.narrow=new FontFace('OrderFace',bytes('{narrow}'));
        globalThis.wide=new FontFace('OrderFace',bytes('{wide}'));
        globalThis.child=frame.contentDocument;
        child.open();child.write('<!doctype html><span id="target" style="display:inline-block;font:40px OrderFace">AAAA</span>');child.close();
        document.fonts.add(narrow);document.fonts.add(wide);
        child.fonts.add(wide);child.fonts.add(narrow);
    "#)).unwrap();
    let widths = |vm: &mut StandaloneScriptVmHarness| {
        vm.eval("[probe.getBoundingClientRect().width,child.getElementById('target').getBoundingClientRect().width].join(',')").unwrap()
    };
    assert_eq!(widths(&mut vm), "160,80");
    vm.eval("document.fonts.delete(narrow);document.fonts.add(narrow)")
        .unwrap();
    assert_eq!(widths(&mut vm), "80,80");
    vm.eval("child.fonts.delete(wide);child.fonts.add(wide)")
        .unwrap();
    assert_eq!(widths(&mut vm), "80,160");
}

#[test]
fn duplicate_css_font_declarations_keep_registration_until_last_member_leaves() {
    let (_, wide) = order_font_data();
    for removed in [0, 1] {
        let mut vm = order_vm();
        vm.eval(&format!(
            r#"
            const style=document.createElement('style');
            const rule='@font-face{{font-family:OrderFace;src:url(data:font/ttf;base64,{wide})}}';
            style.textContent=rule+rule;document.head.append(style);
            globalThis.originalFaces=[...document.fonts];
            document.fonts.load('40px OrderFace','AAAA');
        "#
        ))
        .unwrap();
        assert_eq!(vm.eval("originalFaces.length===2&&originalFaces[0]!==originalFaces[1]&&originalFaces.every(face=>face.status==='loaded')").unwrap(), "true");
        assert_eq!(
            vm.eval("String(probe.getBoundingClientRect().width)")
                .unwrap(),
            "160"
        );
        assert_eq!(
            vm.eval(&format!(
                "document.styleSheets[0].deleteRule({removed});document.fonts.size"
            ))
            .unwrap(),
            "1"
        );
        assert_eq!(
            vm.eval("String(probe.getBoundingClientRect().width)")
                .unwrap(),
            "160",
            "deleting either duplicate must preserve the remaining registration"
        );
        assert_eq!(vm.document_web_font_counts_for_test().2, 1);
        assert_eq!(
            vm.eval("document.styleSheets[0].deleteRule(0);document.fonts.size")
                .unwrap(),
            "0"
        );
        assert_eq!(vm.document_web_font_counts_for_test().2, 0);
    }
}

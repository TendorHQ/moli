use super::*;

fn order_font_data() -> (String, String) {
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

fn order_vm() -> StandaloneScriptVmHarness {
    let mut vm = new_parsed_test_vm(
        "https://font-selection-order.test/",
        "<!doctype html><html><head></head><body><span id='probe' style='display:inline-block;font:40px OrderFace'>AAAA</span><iframe id='frame'></iframe></body></html>",
    );
    vm.set_layout_policy(moli_page_types::LayoutPolicy::OnDemand);
    vm
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

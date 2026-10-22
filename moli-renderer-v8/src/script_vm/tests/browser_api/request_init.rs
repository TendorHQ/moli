use super::*;

#[test]
fn request_and_fetch_share_complete_dictionary_conversion_before_materialization() {
    let mut vm = new_storage_html_test_vm("https://request-init-dictionary.test/");
    let source = format!(
        "globalThis.requestInitResult='pending'; ({}).then(value=>requestInitResult=value,error=>requestInitResult=String(error.stack||error));",
        include_str!("../../../../tests/fixtures/request-init-dictionary.js")
    );
    vm.eval(&source).unwrap();
    vm.eval("0").unwrap();
    assert_eq!(vm.eval("requestInitResult").unwrap(), "true");
}

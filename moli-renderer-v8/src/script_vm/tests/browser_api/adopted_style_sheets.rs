use super::*;

#[test]
fn adopted_stylesheet_setters_convert_iterables_and_preserve_conversion_exceptions() {
    let mut vm = new_parsed_test_vm(
        "https://adopted-stylesheet-iterables.test/",
        "<!doctype html><iframe id=child></iframe>",
    );
    let result = vm
        .eval(include_str!(
            "../../../../tests/fixtures/adopted-style-sheets-iterables.js"
        ))
        .expect("adopted stylesheet conversion matrix should complete");
    assert_eq!(vm.eval("__uiEventResults.total").unwrap(), "180");
    assert_eq!(
        result,
        "true",
        "{}",
        vm.eval("JSON.stringify(__uiEventResults.rows.filter(row => !row.passed))")
            .unwrap(),
    );
}

use super::*;

#[test]
fn input_and_touch_dictionaries_convert_before_native_sequences_and_range_state() {
    let mut vm = new_storage_html_test_vm("https://input-touch-dictionary.test/");
    assert_eq!(
        vm.eval(include_str!(
            "../../../../tests/fixtures/input-touch-dictionary.js"
        ))
        .unwrap(),
        "true"
    );
    assert_eq!(vm.eval(r#"(() => {
        const raw = '\ud800\0\udc00';
        if (new InputEvent('audit', {inputType: raw}).inputType !== raw) throw Error('legacy inputType UTF16');
        for (const sentinel of [{}, null, undefined, 7]) {
            const reads = [];
            let caught = Symbol();
            try {new CapturedMouseEvent('audit', {
                get bubbles() {reads.push('bubbles');return false;},
                get cancelable() {reads.push('cancelable');return false;},
                get composed() {reads.push('composed');return false;},
                get surfaceX() {reads.push('surfaceX');throw sentinel;},
                get surfaceY() {throw Error('must stop');}
            });} catch(e) {caught=e;}
            if (caught !== sentinel || reads.join() !== 'bubbles,cancelable,composed,surfaceX') throw Error('CapturedMouse abrupt completion');
        }
        return true;
    })()"#).unwrap(), "true");
}

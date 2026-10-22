use super::*;

#[test]
fn deferred_constructor_reflection_preserves_identity_and_inheritance() {
    let mut vm = new_storage_page_task_executor_test_vm("https://constructor-reflection.test/");
    let script = include_str!("../../../tests/fixtures/constructor-reflection.js");
    let observed = vm.eval(&format!("JSON.stringify({script})")).unwrap();
    let mut observed: serde_json::Value = serde_json::from_str(&observed).unwrap();
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/constructor-reflection-chromium-145.json"
    ))
    .unwrap();
    let constructors = observed["constructors"].as_array_mut().unwrap();
    let expected_constructors = expected["constructors"].as_array().unwrap();
    assert_eq!(constructors.len(), expected_constructors.len());
    for (row, expected_row) in constructors.iter_mut().zip(expected_constructors) {
        // V8 stringifies callable Proxies anonymously. Only this field may
        // differ from Chromium's named native constructors; the CDP contract
        // runs the same fixture against Chromium and records that difference.
        if row["source"] != expected_row["source"] {
            assert_eq!(row["source"], "function () { [native code] }");
            row["source"] = expected_row["source"].clone();
        }
    }
    assert_eq!(observed, expected);
}

#[test]
fn constructor_realms_follow_bound_targets_and_preserve_revoked_explicit_prototypes() {
    let mut vm = new_storage_page_task_executor_test_vm("https://constructor-realms.test/");
    let script = include_str!("../../../tests/fixtures/constructor-realms.js");
    let observed = vm.eval(&format!("JSON.stringify({script})")).unwrap();
    let observed: serde_json::Value = serde_json::from_str(&observed).unwrap();
    let fallbacks = observed["fallbacks"].as_array().unwrap();
    assert_eq!(fallbacks.len(), 20);
    for row in fallbacks {
        assert_eq!(row["prototypeReads"], 1, "{row}");
        assert_eq!(row["targetInterface"], true, "{row}");
        assert_eq!(row["targetObject"], false, "{row}");
        assert_eq!(row["nativeBrand"], true, "{row}");
    }
    let revoked = observed["revoked"].as_array().unwrap();
    assert_eq!(revoked.len(), 6);
    for row in revoked {
        assert_eq!(row["customPrototype"], true, "{row}");
        assert_eq!(row["nativeBrand"], true, "{row}");
    }
}

#[test]
fn native_constructor_entries_defer_newtarget_lookup_and_use_intrinsic_realm_defaults() {
    let mut vm = new_storage_page_task_executor_test_vm("https://constructor-allocation.test/");
    vm.eval("document.body.innerHTML = '<iframe></iframe>'")
        .unwrap();
    assert_eq!(vm.eval(r#"(() => {
        const assert = (ok, name) => {if (!ok) throw Error(name)};
        const other = document.querySelector('iframe').contentWindow;
        const log = [];
        const N = new Proxy(function(){}, {get(target,key) {
            if (key === 'prototype') log.push('prototype');
            return Reflect.get(target,key);
        }});
        const init = {get candidate() {log.push('candidate'); return ''},
                      get sdpMid() {log.push('sdpMid'); return '0'}};
        const C = RTCIceCandidate;
        const object = Reflect.construct(C, [init], N);
        assert(log.join() === 'candidate,sdpMid,prototype', 'conversion order: '+log);
        assert(Object.getPrototypeOf(object) === N.prototype, 'subclass prototype');
        log.length = 0;
        const marker = {};
        try {Reflect.construct(C, [{get candidate(){throw marker}}], N)}
        catch(error) {assert(error === marker, 'conversion exception identity')}
        assert(log.length === 0, 'failed conversion skipped prototype lookup');
        const parserPrototype = other.DOMParser.prototype;
        const candidatePrototype = other.RTCIceCandidate.prototype;
        const descriptionPrototype = other.RTCSessionDescription.prototype;
        const F = other.Function(''); F.prototype = null;
        const bound = F.bind(null);
        const proxied = new Proxy(bound, {});
        other.DOMParser = other.RTCIceCandidate = other.RTCSessionDescription = function(){throw Error('poisoned')};
        Reflect.construct = () => {throw Error('poisoned Reflect.construct')};
        assert(Object.getPrototypeOf(new DOMParser()) === DOMParser.prototype, 'captured Reflect.construct');
        const construct = other.Reflect.construct;
        assert(Object.getPrototypeOf(construct(DOMParser, [], proxied)) === parserPrototype, 'parser realm default');
        assert(Object.getPrototypeOf(construct(C, [{sdpMid:'0'}], proxied)) === candidatePrototype, 'candidate realm default');
        assert(Object.getPrototypeOf(construct(RTCSessionDescription, [{type:'offer'}], proxied)) === descriptionPrototype, 'description realm default');
        return true;
    })()"#).unwrap(), "true");
}

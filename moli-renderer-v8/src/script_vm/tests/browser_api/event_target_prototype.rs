use super::*;

#[test]
fn event_target_prototype_methods_preserve_native_state_and_realm_semantics() {
    let mut vm = new_parsed_test_vm(
        "https://event-target-prototype.test/",
        "<!doctype html><iframe id=child></iframe>",
    );
    let result = vm
        .eval(include_str!(
            "../../../../tests/fixtures/event-target-prototype.js"
        ))
        .expect("EventTarget prototype matrix should complete");
    assert_eq!(vm.eval("__uiEventResults.total").unwrap(), "158");
    assert_eq!(
        result,
        "true",
        "{}",
        vm.eval("JSON.stringify(__uiEventResults.rows.filter(row => !row.passed))")
            .unwrap(),
    );
}

#[test]
fn event_target_prototype_methods_accept_registered_native_proxies() {
    let mut vm = new_storage_html_test_vm("https://event-target-native-proxy.test/");
    vm.eval(
        r#"
        globalThis.nativeTarget = new EventTarget();
        globalThis.nativeTargetTraps = 0;
        globalThis.nativeTargetProxy = new Proxy(nativeTarget, {
          get() {nativeTargetTraps++; throw new Error('get trap');},
          getPrototypeOf() {nativeTargetTraps++; throw new Error('prototype trap');}
        });
        true;
        "#,
    )
    .unwrap();
    vm.with_default_context_scope_and_checkpoint_for_test(|scope, _host_ptr| {
        let global = scope.get_current_context().global(scope);
        let key = v8::String::new(scope, "nativeTargetProxy").unwrap();
        let value = global.get(scope, key.into()).unwrap();
        let proxy = v8::Local::<v8::Proxy>::try_from(value).unwrap();
        moli_webapi_declare::register_web_api_proxy(scope, proxy).unwrap();
        assert!(crate::web_api_interfaces::EventTarget::is_instance(
            scope,
            proxy.into()
        ));
        Ok(())
    })
    .unwrap();
    assert_eq!(
        vm.eval(
            r#"(() => {
              const {addEventListener: add, removeEventListener: remove, dispatchEvent: dispatch} = EventTarget.prototype;
              let calls = 0;
              function listener(event) {
                if (this === nativeTargetProxy && event.target === nativeTargetProxy && event.currentTarget === nativeTargetProxy) calls++;
              }
              add.call(nativeTargetProxy, 'x', listener);
              if (!dispatch.call(nativeTargetProxy, new Event('x')) || calls !== 1) return false;
              remove.call(nativeTargetProxy, 'x', listener);
              dispatch.call(nativeTargetProxy, new Event('x'));
              if (calls !== 1 || nativeTargetTraps !== 0) return false;
              const author = new Proxy(nativeTargetProxy, {});
              let conversions = 0;
              const type = {toString() {conversions++; return 'x';}};
              for (const call of [() => add.call(author, type, null), () => remove.call(author, type, null), () => dispatch.call(author, new Event('x'))]) {
                try { call(); return false; } catch (error) { if (!(error instanceof TypeError)) return false; }
              }
              return conversions === 0 && nativeTargetTraps === 0;
            })()"#,
        )
        .unwrap(),
        "true",
    );
}

#[test]
fn event_target_prototype_methods_separate_listener_owner_from_dispatch_target() {
    let mut vm = new_storage_html_test_vm("https://event-target-dispatch-identity.test/");
    vm.eval(
        r#"
        globalThis.listenerOwner = new EventTarget();
        globalThis.dispatchTarget = new EventTarget();
        globalThis.identityTraps = 0;
        globalThis.nativeIdentityProxy = new Proxy(listenerOwner, {
            get() {identityTraps++; throw new Error('get trap');},
            getPrototypeOf() {identityTraps++; throw new Error('prototype trap');}
        });
        true;
    "#,
    )
    .unwrap();
    vm.with_default_context_scope_and_checkpoint_for_test(|scope, _| {
        let global = scope.get_current_context().global(scope);
        let owner = global
            .get(scope, crate::util::v8str(scope, "listenerOwner").into())
            .unwrap();
        let owner = v8::Local::<v8::Object>::try_from(owner).unwrap();
        let identity = global
            .get(scope, crate::util::v8str(scope, "dispatchTarget").into())
            .unwrap();
        let identity = v8::Local::<v8::Object>::try_from(identity).unwrap();
        let proxy = global
            .get(
                scope,
                crate::util::v8str(scope, "nativeIdentityProxy").into(),
            )
            .unwrap();
        let proxy = v8::Local::<v8::Proxy>::try_from(proxy).unwrap();
        moli_webapi_declare::register_web_api_proxy(scope, proxy).unwrap();
        crate::context_bootstrap::install_simple_event_target_handlers(scope, owner, &["onprobe"]);
        crate::context_bootstrap::bind_dispatch_target(scope, owner, identity);
        Ok(())
    })
    .unwrap();
    assert_eq!(vm.eval(r#"(() => {
        const assert = (ok, label) => {if (!ok) throw Error(label);};
        const {addEventListener: add, removeEventListener: remove, dispatchEvent: dispatch} = EventTarget.prototype;
        const handler = Object.getOwnPropertyDescriptor(listenerOwner, 'onprobe');
        const calls = [];
        let otherCalls = 0;
        dispatchTarget.addEventListener('probe', () => otherCalls++);
        const observe = (label, receiver, event) => {
            calls.push([label, receiver === dispatchTarget, event.target === dispatchTarget,
                event.currentTarget === dispatchTarget, event.composedPath()[0] === dispatchTarget]);
            event.preventDefault();
        };
        function listener(event) {observe('listener', this, event);}
        const onprobe = function(event) {observe('handler', this, event);};
        const object = {handleEvent(event) {
            observe(this === object ? 'object' : 'wrong-this', event.currentTarget, event);
        }};
        add.call(nativeIdentityProxy, 'probe', listener);
        add.call(listenerOwner, 'probe', listener);
        handler.set.call(nativeIdentityProxy, onprobe);
        add.call(listenerOwner, 'probe', object, {once: true});
        assert(handler.get.call(listenerOwner) === onprobe &&
            handler.get.call(nativeIdentityProxy) === onprobe &&
            handler.get.call(dispatchTarget) === null, 'handler state stays on native receiver');
        const event = new Event('probe', {cancelable: true});
        assert(dispatch.call(nativeIdentityProxy, event) === false, 'cancellation');
        assert(JSON.stringify(calls) === JSON.stringify(['listener','handler','object']
            .map(label => [label,true,true,true,true])), 'listener owner and public identity');
        assert(otherCalls === 0 && event.target === dispatchTarget && event.currentTarget === null &&
            event.composedPath().length === 0, 'dispatch does not use public target registry');
        remove.call(nativeIdentityProxy, 'probe', listener);
        handler.set.call(listenerOwner, null);
        calls.length = 0;
        assert(dispatch.call(listenerOwner, new Event('probe')) && calls.length === 0,
            'once, remove and handler cleanup use listener owner');
        dispatchTarget.dispatchEvent(new Event('probe'));
        assert(otherCalls === 1, 'other registry is untouched');
        const controller = new AbortController();
        add.call(nativeIdentityProxy, 'probe', listener, {signal: controller.signal});
        controller.abort();
        dispatch.call(listenerOwner, new Event('probe'));
        assert(calls.length === 0, 'abort cleanup uses listener owner');
        globalThis.nativeIdentityCalls = 0;
        add.call(nativeIdentityProxy, 'native-probe', function(event) {
            if (this === dispatchTarget && event.target === dispatchTarget &&
                event.currentTarget === dispatchTarget && event.isTrusted) nativeIdentityCalls++;
        }, {once: true});
        return identityTraps === 0;
    })()"#).unwrap(), "true");
    vm.with_default_context_scope_and_checkpoint_for_test(|scope, _| {
        let global = scope.get_current_context().global(scope);
        let proxy = global
            .get(
                scope,
                crate::util::v8str(scope, "nativeIdentityProxy").into(),
            )
            .unwrap();
        let proxy = v8::Local::<v8::Object>::try_from(proxy).unwrap();
        let event =
            crate::context_bootstrap::construct_original_event(scope, "native-probe").unwrap();
        assert!(
            crate::context_bootstrap::dispatch_simple_event_target_event(
                scope,
                proxy,
                "native-probe",
                event,
            )
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(
        vm.eval("JSON.stringify([nativeIdentityCalls, identityTraps])")
            .unwrap(),
        "[1,0]"
    );
}

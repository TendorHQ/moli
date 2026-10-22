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

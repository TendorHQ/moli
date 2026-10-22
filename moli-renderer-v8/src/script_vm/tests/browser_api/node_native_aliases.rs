use super::*;

fn native_alias_handler<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    template: v8::Local<'s, v8::Object>,
) -> v8::Local<'s, v8::Object> {
    let handler = crate::util::new_null_prototype_object(scope);
    for name in ["get", "getPrototypeOf"] {
        let key = crate::util::v8str(scope, name);
        let value = template.get(scope, key.into()).unwrap();
        assert_eq!(
            handler.create_data_property(scope, key.into(), value),
            Some(true)
        );
    }
    handler
}

#[test]
fn native_node_aliases_resolve_backing_without_running_author_traps() {
    let mut vm = new_parsed_test_vm(
        "https://node-native-aliases.test/",
        "<!doctype html><iframe id=child></iframe>",
    );
    vm.eval(
        r#"
        globalThis.aliasTraps = 0;
        globalThis.aliasHandlerTemplate = {
          get() { aliasTraps++; throw Error('get trap'); },
          getPrototypeOf() { aliasTraps++; throw Error('prototype trap'); }
        };
        const other = document.querySelector('#child').contentWindow;
        const detached = document.implementation.createHTMLDocument('');
        globalThis.nodeAliasTargets = [
          document, document.createElement('div'), document.createTextNode('text'),
          document.createDocumentFragment(), detached.createElement('select'),
          detached.createTextNode('detached'), other.document.createElement('div')
        ];
        globalThis.aliasOriginal = nodeAliasTargets[4];
        globalThis.aliasNonNode = new Event('node-alias');
    "#,
    )
    .unwrap();
    vm.with_default_context_scope_and_checkpoint_for_test(|scope, _| {
        let global = scope.get_current_context().global(scope);
        let key = crate::util::v8str(scope, "nodeAliasTargets");
        let targets = global.get(scope, key.into()).unwrap();
        let targets = v8::Local::<v8::Array>::try_from(targets).unwrap();
        let key = crate::util::v8str(scope, "aliasHandlerTemplate");
        let template = global.get(scope, key.into()).unwrap();
        let template = v8::Local::<v8::Object>::try_from(template).unwrap();
        let key = crate::util::v8str(scope, "aliasNonNode");
        let non_node = global.get(scope, key.into()).unwrap();
        let non_node = v8::Local::<v8::Object>::try_from(non_node).unwrap();
        let handler = native_alias_handler(scope, template);
        let non_node = v8::Proxy::new(scope, non_node, handler).unwrap();
        moli_webapi_declare::register_web_api_proxy(scope, non_node).unwrap();
        assert!(crate::web_api_interfaces::Event::is_instance(
            scope,
            non_node.into()
        ));
        assert!(
            crate::native_bridge::node_runtime_and_handle_from_object(scope, non_node.into())
                .is_err(),
        );
        assert!(
            crate::native_bridge::node_runtime_and_handle_from_object_or_detached(
                scope,
                non_node.into(),
            )
            .is_err(),
        );
        for index in 0..targets.length() {
            let target = targets.get_index(scope, index).unwrap();
            let target = v8::Local::<v8::Object>::try_from(target).unwrap();
            let expected = crate::native_bridge::node_runtime_and_handle_from_object_or_detached(
                scope, target,
            )
            .unwrap();
            let backing = moli_webapi_declare::web_api_object_target(scope, target).unwrap();
            let handler = native_alias_handler(scope, template);
            let proxy = v8::Proxy::new(scope, backing, handler).unwrap();
            moli_webapi_declare::register_web_api_proxy(scope, proxy).unwrap();
            assert_eq!(
                crate::native_bridge::node_runtime_and_handle_from_object_or_detached(
                    scope,
                    proxy.into(),
                )
                .unwrap(),
                expected,
                "native alias {index}",
            );
            let author = v8::Proxy::new(scope, proxy.into(), handler).unwrap();
            assert!(
                crate::native_bridge::node_runtime_and_handle_from_object_or_detached(
                    scope,
                    author.into(),
                )
                .is_err(),
                "author wrapper {index}",
            );
            author.revoke();
            assert!(
                crate::native_bridge::node_runtime_and_handle_from_object_or_detached(
                    scope,
                    author.into(),
                )
                .is_err(),
                "revoked wrapper {index}",
            );
            if index == 4 {
                let key = crate::util::v8str(scope, "aliasBeforeAdoption");
                assert_eq!(
                    global.create_data_property(scope, key.into(), proxy.into()),
                    Some(true)
                );
            }
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(vm.eval("aliasTraps").unwrap(), "0");
    assert_eq!(
        vm.eval("document.adoptNode(aliasOriginal); document.body.appendChild(aliasOriginal); aliasOriginal.ownerDocument === document && aliasOriginal.isConnected").unwrap(),
        "true",
    );
    vm.with_default_context_scope_and_checkpoint_for_test(|scope, host_ptr| {
        let global = scope.get_current_context().global(scope);
        let key = crate::util::v8str(scope, "aliasOriginal");
        let original = global.get(scope, key.into()).unwrap();
        let original = v8::Local::<v8::Object>::try_from(original).unwrap();
        let key = crate::util::v8str(scope, "aliasBeforeAdoption");
        let alias = global.get(scope, key.into()).unwrap();
        let alias = v8::Local::<v8::Object>::try_from(alias).unwrap();
        let expected =
            crate::native_bridge::node_runtime_and_handle_from_object_or_detached(scope, original)
                .unwrap();
        assert_eq!(
            expected.0, host_ptr,
            "adoption reaches the current native host"
        );
        let host = unsafe { &*host_ptr };
        assert_eq!(
            host.dom_host().owner_document_handle(expected.1),
            Some(host.document_handle())
        );
        assert!(host.dom_host().is_connected_to_document(expected.1));
        assert_eq!(
            crate::native_bridge::node_runtime_and_handle_from_object_or_detached(scope, alias)
                .unwrap(),
            expected,
            "an alias created before adoption follows the canonical native delegate",
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(vm.eval("aliasTraps").unwrap(), "0");
}

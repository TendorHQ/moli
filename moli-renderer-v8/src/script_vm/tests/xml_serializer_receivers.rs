use super::*;

#[test]
fn xml_serializer_native_receivers_reject_forgery_and_author_proxies_in_the_callee_realm() {
    let mut vm = new_storage_html_test_vm("https://xml-serializer-receivers.test/");
    let result = vm.eval(r#"
        document.body.innerHTML = '<iframe></iframe>';
        const other = document.querySelector('iframe').contentWindow;
        const serializer = new XMLSerializer();
        const node = document.createElement('div');
        let traps = 0;
        const author = new Proxy(serializer, {
            get() { traps++; throw new Error('unexpected get trap'); },
            getPrototypeOf() { traps++; throw new Error('unexpected prototype trap'); }
        });
        const revoked = Proxy.revocable(serializer, {}); revoked.revoke();
        for (const realm of [window, other]) {
            const serialize = realm.XMLSerializer.prototype.serializeToString;
            if (serialize.call(serializer, node) !== serialize.call(new realm.XMLSerializer(), node))
                throw new Error('genuine cross-realm serializer');
            for (const receiver of [null, undefined, {}, Object.create(serializer),
                                    Object.create(XMLSerializer.prototype), author, revoked.proxy]) {
                let error;
                try { serialize.call(receiver, node); } catch (caught) { error = caught; }
                if (!error || Object.getPrototypeOf(error) !== realm.TypeError.prototype)
                    throw new Error('invalid receiver must throw the callee TypeError');
            }
        }
        if (traps !== 0) throw new Error('receiver validation invoked an author Proxy trap');
        'ok';
    "#).unwrap();
    assert_eq!(result, "ok");
}

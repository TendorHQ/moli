use crate::webidl;

#[derive(Default, webidl::WebIdlDictionary)]
#[webidl(prefix = "EventInit", snapshot)]
pub(in crate::context_bootstrap) struct EventInit {
    #[webidl(default = false)]
    pub(in crate::context_bootstrap) bubbles: bool,
    #[webidl(default = false)]
    pub(in crate::context_bootstrap) cancelable: bool,
    #[webidl(default = false)]
    pub(in crate::context_bootstrap) composed: bool,
}

pub(in crate::context_bootstrap) fn parse_event_dictionary<'s, T: webidl::WebIdlDictionary<'s>>(
    scope: &mut v8::PinScope<'s, '_>,
    args: &v8::FunctionCallbackArguments<'s>,
    constructor: &'static str,
) -> Option<T> {
    let parsed = webidl::convert::<webidl::Dictionary<T>>(
        scope,
        args.get(1),
        webidl::Context::argument(constructor, 2),
    );
    match parsed {
        Ok(init) => Some(init.0),
        Err(error) => {
            webidl::throw_error(scope, &error);
            None
        }
    }
}

pub(super) fn init_bool_property<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    init: Option<v8::Local<'s, v8::Object>>,
    key: &'static str,
    default: bool,
) -> bool {
    let Some(object) = init else {
        return default;
    };
    webidl::legacy_bool_member_or(scope, object, "EventInit", key, default)
}

pub(super) fn init_string_property<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    init: Option<v8::Local<'s, v8::Object>>,
    key: &'static str,
    default: &str,
) -> String {
    let Some(object) = init else {
        return default.to_owned();
    };
    webidl::legacy_string_member_or(scope, object, "EventInit", key, default)
}

pub(super) fn init_value_property<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    init: Option<v8::Local<'s, v8::Object>>,
    key: &'static str,
) -> Option<v8::Local<'s, v8::Value>> {
    init.and_then(|object| webidl::property_non_undefined(scope, object, key))
}

pub(super) fn read_event_init<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: &v8::FunctionCallbackArguments<'s>,
) -> Option<(bool, bool, bool)> {
    parse_event_dictionary::<EventInit>(scope, args, "Event")
        .map(|init| (init.bubbles, init.cancelable, init.composed))
}

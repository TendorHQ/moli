use super::*;
use crate::context_bootstrap::shared_event_targets::{
    ResolvedEventTarget, resolve_event_target, shared_target_owner,
};
use crate::util::{get_private_object, new_null_prototype_object, set_private_value};

pub(crate) fn initialize_simple_event_target<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
) {
    let target = shared_target_owner(scope, target);
    if get_private_object(scope, target, SIMPLE_EVENT_TARGET_LISTENERS_SLOT).is_none() {
        let registry = new_null_prototype_object(scope);
        set_private_value(
            scope,
            target,
            SIMPLE_EVENT_TARGET_LISTENERS_SLOT,
            registry.into(),
        );
    }
}

pub(crate) fn is_simple_event_target<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
) -> bool {
    resolve_simple_event_target(scope, target).is_some()
}

pub(super) fn resolve_simple_event_target<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
) -> Option<ResolvedEventTarget<'s>> {
    // Resolve registered native Proxies only for the simple-target branch.
    // DOM and Window targets keep their existing receiver resolution.
    let target = resolve_event_target(scope, target)?;
    get_private_object(
        scope,
        target.listener_owner,
        SIMPLE_EVENT_TARGET_LISTENERS_SLOT,
    )?;
    Some(target)
}

pub(super) fn simple_object_event_listener_registry<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
) -> Option<v8::Local<'s, v8::Object>> {
    let target = shared_target_owner(scope, target);
    // Read only the target's own private state. Looking through prototypes would
    // let instances inherit another target's listener list and receiver identity.
    get_private_object(scope, target, SIMPLE_EVENT_TARGET_LISTENERS_SLOT)
}

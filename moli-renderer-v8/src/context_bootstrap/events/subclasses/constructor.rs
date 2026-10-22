use super::super::message;
use super::*;
use crate::web_api_interfaces;

fn event_subclass_kind_from_callback_data(
    data: v8::Local<'_, v8::Value>,
) -> Option<EventSubclassKind> {
    let value = i32::try_from(v8::Local::<v8::Integer>::try_from(data).ok()?.value()).ok()?;
    EventSubclassKind::from_i32(value)
}

fn event_subclass_constructor_callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let Some(kind) = event_subclass_kind_from_callback_data(args.data()) else {
        throw_type_error(scope, "Invalid event subclass constructor.");
        return;
    };

    if !args.is_construct_call() {
        throw_type_error(
            scope,
            &format!(
                "Failed to construct '{}': Please use the 'new' operator.",
                kind.constructor_name()
            ),
        );
        return;
    }

    if matches!(
        kind,
        EventSubclassKind::PromiseRejectionEvent | EventSubclassKind::FormDataEvent
    ) && args.length() < 2
    {
        throw_type_error(
            scope,
            &format!(
                "Failed to construct '{}': 2 arguments required, but only {} present.",
                kind.constructor_name(),
                args.length()
            ),
        );
        return;
    }

    let wrapper = args.this();
    let Some(event_type) = event_type_argument(scope, &args, kind.constructor_name()) else {
        return;
    };
    let init = {
        let init_arg = args.get(1);
        if init_arg.is_null_or_undefined() || !init_arg.is_object() {
            None
        } else {
            init_arg.to_object(scope)
        }
    };
    let basic_event_init = if matches!(
        kind,
        EventSubclassKind::UiEvent
            | EventSubclassKind::FocusEvent
            | EventSubclassKind::TextEvent
            | EventSubclassKind::CompositionEvent
            | EventSubclassKind::CustomEvent
    ) {
        let Some(init) = basic::BasicEventInit::parse(scope, &args, kind) else {
            return;
        };
        Some(init)
    } else {
        None
    };
    let pointer_event_init = if matches!(
        kind,
        EventSubclassKind::MouseEvent
            | EventSubclassKind::WheelEvent
            | EventSubclassKind::PointerEvent
            | EventSubclassKind::DragEvent
    ) {
        let Some(init) = pointer::PointerEventInitPayload::parse(scope, &args, kind) else {
            return;
        };
        Some(init)
    } else {
        None
    };
    let interaction_event_init = if matches!(
        kind,
        EventSubclassKind::InputEvent
            | EventSubclassKind::TouchEvent
            | EventSubclassKind::CapturedMouseEvent
    ) {
        let Some(init) = input::InteractionEventInit::parse(scope, &args, kind) else {
            return;
        };
        Some(init)
    } else {
        None
    };
    let payload_event_init = if matches!(
        kind,
        EventSubclassKind::PromiseRejectionEvent
            | EventSubclassKind::FormDataEvent
            | EventSubclassKind::SubmitEvent
            | EventSubclassKind::TrackEvent
    ) {
        let Some(init) = payload::PayloadEventInit::parse(scope, &args, kind) else {
            return;
        };
        Some(init)
    } else {
        None
    };
    let value_event_init = if matches!(
        kind,
        EventSubclassKind::ClipboardEvent
            | EventSubclassKind::CloseEvent
            | EventSubclassKind::CommandEvent
            | EventSubclassKind::ToggleEvent
            | EventSubclassKind::InterestEvent
            | EventSubclassKind::PopStateEvent
            | EventSubclassKind::HashChangeEvent
            | EventSubclassKind::PageTransitionEvent
    ) {
        let Some(init) = values::ValueEventInit::parse(scope, &args, kind) else {
            return;
        };
        Some(init)
    } else {
        None
    };
    let font_face_init = if kind == EventSubclassKind::FontFaceSetLoadEvent {
        let Some(init) = super::super::parse_event_dictionary::<
            crate::context_bootstrap::css_fontface_runtime::FontFaceSetLoadEventInit,
        >(scope, &args, "FontFaceSetLoadEvent") else {
            return;
        };
        Some(init)
    } else {
        None
    };
    let storage_event_init = if kind == EventSubclassKind::StorageEvent {
        let Some(init) = data::parse_storage_event_init(scope, &args) else {
            return;
        };
        Some(init)
    } else {
        None
    };
    let keyboard_event_init = if kind == EventSubclassKind::KeyboardEvent {
        let Some(init) = keyboard::parse_keyboard_event_init(scope, &args) else {
            return;
        };
        Some(init)
    } else {
        None
    };
    let security_policy_init = if kind == EventSubclassKind::SecurityPolicyViolationEvent {
        let Some(init) = security_policy::parse_security_policy_violation_event_init(scope, &args)
        else {
            return;
        };
        Some(init)
    } else {
        None
    };
    let message_event_init = if kind == EventSubclassKind::MessageEvent {
        let Some(init) = message::parse_message_event_init(scope, &args) else {
            return;
        };
        Some(init)
    } else {
        None
    };
    let error_init = if kind == EventSubclassKind::ErrorEvent {
        let Some(init) = error::parse_error_event_init(scope, &args) else {
            return;
        };
        Some(init)
    } else {
        None
    };
    let Some((bubbles, cancelable, composed)) = security_policy_init
        .as_ref()
        .map(security_policy::SecurityPolicyViolationEventInit::event_flags)
        .or_else(|| error_init.as_ref().map(error::ErrorEventInit::event_flags))
        .or_else(|| {
            keyboard_event_init
                .as_ref()
                .map(keyboard::KeyboardEventInit::event_flags)
        })
        .or_else(|| {
            storage_event_init
                .as_ref()
                .map(data::StorageEventInitMembers::event_flags)
        })
        .or_else(|| {
            message_event_init
                .as_ref()
                .map(message::MessageEventInit::event_flags)
        })
        .or_else(|| {
            basic_event_init
                .as_ref()
                .map(basic::BasicEventInit::event_flags)
        })
        .or_else(|| {
            pointer_event_init
                .as_ref()
                .map(pointer::PointerEventInitPayload::event_flags)
        })
        .or_else(|| font_face_init.as_ref().map(crate::context_bootstrap::css_fontface_runtime::FontFaceSetLoadEventInit::event_flags))
        .or_else(|| value_event_init.as_ref().map(values::ValueEventInit::event_flags))
        .or_else(|| interaction_event_init.as_ref().map(input::InteractionEventInit::event_flags))
        .or_else(|| payload_event_init.as_ref().map(payload::PayloadEventInit::event_flags))
        .or_else(|| read_event_init(scope, &args))
    else {
        return;
    };

    let event = new_event_state(scope);

    initialize_event_object_with_type(scope, event, event_type, bubbles, cancelable);
    define_event_property(
        scope,
        event,
        "composed",
        v8::Boolean::new(scope, composed).into(),
    );

    match kind {
        EventSubclassKind::UiEvent
        | EventSubclassKind::FocusEvent
        | EventSubclassKind::TextEvent
        | EventSubclassKind::CompositionEvent
        | EventSubclassKind::CustomEvent => {
            basic_event_init
                .expect("basic event init is parsed")
                .initialize(scope, event);
        }
        EventSubclassKind::MouseEvent
        | EventSubclassKind::WheelEvent
        | EventSubclassKind::PointerEvent
        | EventSubclassKind::DragEvent => {
            pointer_event_init
                .expect("pointing event init is parsed")
                .initialize(scope, event);
        }
        EventSubclassKind::ClipboardEvent
        | EventSubclassKind::CloseEvent
        | EventSubclassKind::CommandEvent
        | EventSubclassKind::ToggleEvent
        | EventSubclassKind::InterestEvent
        | EventSubclassKind::PopStateEvent
        | EventSubclassKind::HashChangeEvent
        | EventSubclassKind::PageTransitionEvent => value_event_init
            .expect("value event init is parsed")
            .initialize(scope, event),
        EventSubclassKind::InputEvent
        | EventSubclassKind::TouchEvent
        | EventSubclassKind::CapturedMouseEvent => {
            if !interaction_event_init
                .expect("interaction event init is parsed")
                .initialize(scope, event)
            {
                return;
            }
        }
        EventSubclassKind::KeyboardEvent => {
            keyboard::initialize_keyboard_event(
                scope,
                event,
                keyboard_event_init.expect("KeyboardEvent init should be parsed"),
            );
        }
        EventSubclassKind::MessageEvent => message::initialize_message_event(
            scope,
            event,
            message_event_init.expect("MessageEvent init should be parsed"),
        ),
        EventSubclassKind::StorageEvent => {
            data::initialize_storage_event(
                scope,
                event,
                storage_event_init.expect("StorageEvent init should be parsed"),
            );
        }
        EventSubclassKind::ErrorEvent => error_init
            .expect("ErrorEvent init is parsed")
            .initialize(scope, event),
        EventSubclassKind::SecurityPolicyViolationEvent => {
            security_policy_init
                .expect("SecurityPolicyViolationEvent init is parsed")
                .initialize(scope, event);
        }
        EventSubclassKind::PromiseRejectionEvent
        | EventSubclassKind::FormDataEvent
        | EventSubclassKind::SubmitEvent
        | EventSubclassKind::TrackEvent => {
            payload_event_init
                .expect("payload event init is parsed")
                .initialize(scope, event);
        }
        EventSubclassKind::NavigationCurrentEntryChangeEvent => {
            if !data::initialize_navigation_current_entry_change_event(scope, event, init) {
                return;
            }
        }
        EventSubclassKind::NavigateEvent => {
            if !data::initialize_navigate_event(scope, event, init) {
                return;
            }
        }
        EventSubclassKind::FontFaceSetLoadEvent => {
            font_face_init
                .expect("font event init is parsed")
                .initialize(scope, event);
        }
    }

    web_api_interfaces::initialize(scope, event, kind.constructor_name())
        .expect("event primary interface should initialize");
    set_private_value(
        scope,
        event,
        EVENT_SUBCLASS_KIND_SLOT,
        v8::Integer::new(scope, kind as i32).into(),
    );
    if initialize_event_wrapper(scope, wrapper, event).is_some() {
        rv.set(wrapper.into());
    }
}

pub(in crate::context_bootstrap) fn build_event_subclass_template<'s>(
    scope: &mut v8::PinScope<'s, '_, ()>,
    kind: EventSubclassKind,
) -> v8::Local<'s, v8::FunctionTemplate> {
    let data: v8::Local<'s, v8::Value> = v8::Integer::new(scope, kind as i32).into();
    let length = if matches!(
        kind,
        EventSubclassKind::NavigateEvent
            | EventSubclassKind::PromiseRejectionEvent
            | EventSubclassKind::FormDataEvent
    ) {
        2
    } else {
        1
    };
    v8::FunctionTemplate::builder(event_subclass_constructor_callback)
        .data(data)
        .length(length)
        .build(scope)
}

//! HTML directionality is DOM semantics, independent of computed CSS direction
//! and flattened layout ancestry. This is the shared source for `:dir()` and
//! consumers such as form submission and presentation hints.

use crate::{
    NodeId,
    native::{DomHost, Element, Node},
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlDirection {
    Ltr,
    Rtl,
}

impl HtmlDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ltr => "ltr",
            Self::Rtl => "rtl",
        }
    }
}

pub fn first_strong_text_direction(value: &str) -> Option<HtmlDirection> {
    value
        .chars()
        .find_map(|ch| match unicode_bidi::bidi_class(ch) {
            unicode_bidi::BidiClass::L => Some(HtmlDirection::Ltr),
            unicode_bidi::BidiClass::R | unicode_bidi::BidiClass::AL => Some(HtmlDirection::Rtl),
            _ => None,
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlDir {
    Undefined,
    Explicit(HtmlDirection),
    Auto,
}

impl HtmlDir {
    pub fn for_element(element: &Element) -> Self {
        if element.namespace() != "http://www.w3.org/1999/xhtml" {
            return Self::Undefined;
        }
        match element.attribute_ns("", "dir") {
            Some(value) if value.eq_ignore_ascii_case("ltr") => Self::Explicit(HtmlDirection::Ltr),
            Some(value) if value.eq_ignore_ascii_case("rtl") => Self::Explicit(HtmlDirection::Rtl),
            Some(value) if value.eq_ignore_ascii_case("auto") => Self::Auto,
            _ => Self::Undefined,
        }
    }
}

// A slot encountered during a contained-text scan contributes its shadow
// host's *direction*, not its fallback text. Carry that dependency back to
// the outer walk so nested shadow trees never require recursive resolution.
enum AutoDirectionality {
    Text(Option<HtmlDirection>),
    ShadowHost(NodeId),
}

/// Resolve HTML directionality without consulting CSS or the layout tree.
pub fn html_directionality(host: &DomHost, handle: NodeId) -> HtmlDirection {
    let mut current = Some(handle);
    while let Some(handle) = current {
        match directionality_dependency(host, handle) {
            DirectionalityDependency::Resolved(direction) => return direction,
            DirectionalityDependency::Element(parent) => current = parent,
        }
    }
    HtmlDirection::Ltr
}

enum DirectionalityDependency {
    Resolved(HtmlDirection),
    Element(Option<NodeId>),
}

fn directionality_dependency(host: &DomHost, handle: NodeId) -> DirectionalityDependency {
    if let Some(element) = host.node(handle).and_then(Node::as_element) {
        let dir = HtmlDir::for_element(element);
        if let HtmlDir::Explicit(direction) = dir {
            return DirectionalityDependency::Resolved(direction);
        }
        if dir == HtmlDir::Auto || element.is_html_element("bdi") {
            return match auto_directionality(host, handle, element) {
                AutoDirectionality::Text(direction) => {
                    DirectionalityDependency::Resolved(direction.unwrap_or(HtmlDirection::Ltr))
                }
                AutoDirectionality::ShadowHost(host) => {
                    DirectionalityDependency::Element(Some(host))
                }
            };
        }
        if element.is_html_input() && element.input_type() == "tel" {
            return DirectionalityDependency::Resolved(HtmlDirection::Ltr);
        }
    }
    DirectionalityDependency::Element(
        host.parent_node(handle)
            .or_else(|| host.shadow_root_host(handle)),
    )
}

/// A read-only batch resolver. Its memo is local to one side of a mutation;
/// no semantic cache survives a DOM write. An inherited subtree resolves its
/// shared auto root once, rather than rescanning the same text for every child.
#[derive(Default)]
pub(crate) struct DirectionalityResolver {
    resolved: HashMap<NodeId, HtmlDirection>,
}

impl DirectionalityResolver {
    pub(crate) fn resolve(&mut self, host: &DomHost, handle: NodeId) -> HtmlDirection {
        let mut path = Vec::new();
        let mut current = Some(handle);
        let direction = loop {
            let Some(node) = current else {
                break HtmlDirection::Ltr;
            };
            if let Some(&direction) = self.resolved.get(&node) {
                break direction;
            }
            path.push(node);
            match directionality_dependency(host, node) {
                DirectionalityDependency::Resolved(direction) => break direction,
                DirectionalityDependency::Element(parent) => current = parent,
            }
        };
        for node in path {
            self.resolved.insert(node, direction);
        }
        direction
    }
}

fn auto_directionality(host: &DomHost, root: NodeId, element: &Element) -> AutoDirectionality {
    if element.is_html_textarea()
        || (element.is_html_input() && input_uses_value_for_auto_direction(&element.input_type()))
    {
        let value = if element.is_html_textarea() && !element.input_value_dirty() {
            host.dom().direct_text_content(root).unwrap_or_default()
        } else {
            element.input_value()
        };
        return AutoDirectionality::Text(first_strong_text_direction(&value));
    }

    if element.is_html_element("slot") && host.containing_shadow_root(root).is_some() {
        let assigned = host.assigned_nodes_for_slot_with_options(root, false);
        if !assigned.is_empty() {
            for child in assigned {
                let Some(node) = host.node(child) else {
                    continue;
                };
                let direction = if let Some(text) = node.as_text() {
                    AutoDirectionality::Text(first_strong_text_direction(text.data()))
                } else if node
                    .as_element()
                    .is_some_and(|element| !excludes_auto_text(element))
                {
                    contained_text_directionality(host, child)
                } else {
                    continue;
                };
                if !matches!(direction, AutoDirectionality::Text(None)) {
                    return direction;
                }
            }
            return AutoDirectionality::Text(None);
        }
    }
    contained_text_directionality(host, root)
}

fn contained_text_directionality(host: &DomHost, root: NodeId) -> AutoDirectionality {
    let mut stack = host.child_handles_reversed(root).collect::<Vec<_>>();
    while let Some(handle) = stack.pop() {
        let Some(node) = host.node(handle) else {
            continue;
        };
        if let Some(text) = node.as_text() {
            if let Some(direction) = first_strong_text_direction(text.data()) {
                return AutoDirectionality::Text(Some(direction));
            }
            continue;
        }
        let Some(element) = node.as_element() else {
            continue;
        };
        if excludes_auto_text(element) {
            continue;
        }
        if element.is_html_element("slot")
            && let Some(shadow_root) = host.containing_shadow_root(handle)
            && let Some(shadow_host) = host.shadow_root_host(shadow_root)
        {
            return AutoDirectionality::ShadowHost(shadow_host);
        }
        stack.extend(host.child_handles_reversed(handle));
    }
    AutoDirectionality::Text(None)
}

pub(crate) fn excludes_auto_text(element: &Element) -> bool {
    ["bdi", "script", "style", "textarea"]
        .iter()
        .any(|name| element.is_html_element(name))
        || HtmlDir::for_element(element) != HtmlDir::Undefined
}

fn input_uses_value_for_auto_direction(input_type: &str) -> bool {
    matches!(
        input_type,
        "hidden"
            | "text"
            | "search"
            | "tel"
            | "url"
            | "email"
            | "password"
            | "submit"
            | "reset"
            | "button"
    )
}

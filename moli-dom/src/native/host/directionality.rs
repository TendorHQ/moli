//! Mutation-local snapshots of HTML directionality dependencies. HTML inherits
//! through DOM parents, not CSS's flat tree; only auto slots inspect assignees.

use indexmap::IndexMap;

use super::{DomHandle, DomHost, DomMutationEffects, Element, Node};
use crate::directionality::{DirectionalityResolver, HtmlDir, HtmlDirection, excludes_auto_text};

pub(super) fn has_directionality_source(element: &Element) -> bool {
    element.is_html_element("bdi")
        || (element.namespace() == "http://www.w3.org/1999/xhtml"
            && element.attribute_ns("", "dir").is_some())
}

#[derive(Default)]
pub(super) struct DirectionalitySnapshot {
    values: IndexMap<DomHandle, HtmlDirection>,
    resolver: DirectionalityResolver,
}

impl DirectionalitySnapshot {
    pub(super) fn for_text(host: &DomHost, node: DomHandle) -> Self {
        let mut snapshot = Self::default();
        if !host.directionality_may_vary.get() {
            return snapshot;
        }
        snapshot.capture_text_dependents(host, node);
        snapshot
    }

    pub(super) fn for_tree(host: &DomHost, parent: DomHandle, child: DomHandle) -> Self {
        let mut snapshot = Self::default();
        // Detached parser/import trees can introduce a source without having
        // passed through an attribute mutation. Inspect only the inserted tree,
        // never the document or an ever-growing ancestor chain in plain LTR DOM.
        if !host.directionality_may_vary.get() {
            let mut stack = vec![child];
            let has_source = |node| {
                host.node(node)
                    .and_then(Node::as_element)
                    .is_some_and(has_directionality_source)
            };
            let mut found = has_source(parent);
            while !found && let Some(node) = stack.pop() {
                found = has_source(node);
                stack.extend(host.child_handles_reversed(node));
                if let Some(shadow) = host.shadow_root_handle(node) {
                    stack.push(shadow);
                }
            }
            if !found {
                return snapshot;
            }
            host.directionality_may_vary.set(true);
        }
        snapshot.capture_subtree(host, child);
        snapshot.capture_text_dependents(host, parent);
        snapshot.capture_slot_dependents(host, parent);
        if let Some(old_parent) = host.parent_node(child) {
            snapshot.capture_text_dependents(host, old_parent);
            snapshot.capture_slot_dependents(host, old_parent);
        }
        snapshot
    }

    pub(super) fn for_attribute(
        host: &DomHost,
        node: DomHandle,
        namespace: Option<&str>,
        name: &str,
    ) -> Self {
        let mut snapshot = Self::default();
        if namespace.is_some_and(|namespace| !namespace.is_empty()) {
            return snapshot;
        }
        let Some(element) = host.node(node).and_then(Node::as_element) else {
            return snapshot;
        };
        // A foreign element can be assigned to an HTML slot as well. Its slot
        // attribute affects the slot's auto direction, not its own HTML dir.
        if element.normalized_attribute_name(name) == "slot" {
            if host.directionality_may_vary.get() {
                snapshot.capture_slot_dependents(host, node);
            }
            return snapshot;
        }
        if element.namespace() != "http://www.w3.org/1999/xhtml" {
            return snapshot;
        }
        if name.eq_ignore_ascii_case("dir") {
            host.directionality_may_vary.set(true);
        }
        if !host.directionality_may_vary.get() {
            return snapshot;
        }
        match element.normalized_attribute_name(name).as_str() {
            "dir" => {
                snapshot.capture_subtree(host, node);
                if let Some(slot) = host.assigned_slot_for_node(node) {
                    snapshot.capture_auto_slot(host, slot);
                }
                if let Some(parent) = host.parent_node(node) {
                    snapshot.capture_text_dependents(host, parent);
                }
            }
            "type" | "value" if element.is_html_input() => snapshot.capture_subtree(host, node),
            "name" => snapshot.capture_slot_dependents(host, node),
            _ => {}
        }
        snapshot
    }

    pub(super) fn for_value(host: &DomHost, node: DomHandle) -> Self {
        let mut snapshot = Self::default();
        if !host.directionality_may_vary.get() {
            return snapshot;
        }
        snapshot.capture_subtree(host, node);
        snapshot
    }

    pub(super) fn for_slots(host: &DomHost, slots: impl IntoIterator<Item = DomHandle>) -> Self {
        let mut snapshot = Self::default();
        if !host.directionality_may_vary.get() {
            return snapshot;
        }
        for slot in slots {
            snapshot.capture_auto_slot(host, slot);
        }
        snapshot
    }

    fn capture_text_dependents(&mut self, host: &DomHost, node: DomHandle) {
        let mut current = Some(node);
        while let Some(node) = current {
            if let Some(element) = host.node(node).and_then(Node::as_element)
                && excludes_auto_text(element)
            {
                if HtmlDir::for_element(element) == HtmlDir::Auto || element.is_html_element("bdi")
                {
                    self.capture_subtree(host, node);
                }
                break;
            }
            if let Some(slot) = host.assigned_slot_for_node(node) {
                self.capture_auto_slot(host, slot);
            }
            // An ancestor's contained-text scan never crosses a shadow root.
            current = host
                .parent_node(node)
                .filter(|parent| !host.is_shadow_root(*parent));
        }
    }

    fn capture_auto_slot(&mut self, host: &DomHost, slot: DomHandle) {
        if host
            .node(slot)
            .and_then(Node::as_element)
            .is_some_and(|element| HtmlDir::for_element(element) == HtmlDir::Auto)
        {
            self.capture_subtree(host, slot);
        }
    }

    fn capture_slot_dependents(&mut self, host: &DomHost, node: DomHandle) {
        let shadow = host
            .shadow_root_handle(node)
            .or_else(|| {
                host.parent_node(node)
                    .and_then(|parent| host.shadow_root_handle(parent))
            })
            .or_else(|| host.containing_shadow_root(node));
        let Some(shadow) = shadow else { return };
        let mut stack = vec![shadow];
        while let Some(node) = stack.pop() {
            if host.is_html_element_named(node, "slot") {
                self.capture_auto_slot(host, node);
            }
            stack.extend(host.child_handles_reversed(node));
        }
    }

    fn capture_subtree(&mut self, host: &DomHost, root: DomHandle) {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if self.values.contains_key(&node) {
                continue;
            }
            if host.node(node).is_some_and(Node::is_element) {
                let direction = self.resolver.resolve(host, node);
                self.values.insert(node, direction);
            }
            stack.extend(host.child_handles_reversed(node));
            if let Some(shadow) = host.shadow_root_handle(node) {
                stack.push(shadow);
            }
        }
    }

    pub(super) fn finish(self, host: &DomHost, effects: &mut DomMutationEffects) {
        let mut resolver = DirectionalityResolver::default();
        for (node, before) in self.values {
            if host.node(node).is_none() {
                continue;
            }
            let after = resolver.resolve(host, node);
            effects.mark_directionality_change(node, before, after);
        }
    }
}

#[cfg(test)]
mod tests;

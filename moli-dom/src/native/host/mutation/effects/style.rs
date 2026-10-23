use std::sync::Arc;

use super::{DomHandle, DomMutationEffects};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DomStyleInvalidationInputs {
    pub(super) attribute_mutations: Vec<DomAttributeMutation>,
    pub(super) child_list_mutations: Vec<DomChildListMutation>,
    pub(super) character_data_mutations: Vec<DomCharacterDataMutation>,
}

impl DomStyleInvalidationInputs {
    pub fn attribute_mutations(&self) -> &[DomAttributeMutation] {
        &self.attribute_mutations
    }

    pub fn child_list_mutations(&self) -> &[DomChildListMutation] {
        &self.child_list_mutations
    }

    pub fn character_data_mutations(&self) -> &[DomCharacterDataMutation] {
        &self.character_data_mutations
    }

    fn push_character_data_mutation(&mut self, mutation: DomCharacterDataMutation) {
        if let Some(previous) = self
            .character_data_mutations
            .iter_mut()
            .find(|previous| previous.target == mutation.target)
        {
            previous.new_is_empty = mutation.new_is_empty;
        } else {
            self.character_data_mutations.push(mutation);
        }
    }

    pub(super) fn push_attribute_mutation(&mut self, mutation: DomAttributeMutation) {
        if !self.attribute_mutations.contains(&mutation) {
            self.attribute_mutations.push(mutation);
        }
    }

    fn push_child_list_mutation(&mut self, mutation: DomChildListMutation) {
        if !self.child_list_mutations.contains(&mutation) {
            self.child_list_mutations.push(mutation);
        }
    }

    pub(super) fn merge(&mut self, other: Self) {
        for mutation in other.attribute_mutations {
            self.push_attribute_mutation(mutation);
        }
        for mutation in other.character_data_mutations {
            self.push_character_data_mutation(mutation);
        }
        for mutation in other.child_list_mutations {
            self.push_child_list_mutation(mutation);
        }
    }
}

/// A text/CDATA write and its selector-relevant content transition. Retain
/// every changed target for script notifications, but only an emptiness
/// transition can change structural selectors such as `:empty`. HTML
/// directionality and stylesheet source text have separate effect owners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomCharacterDataMutation {
    target: DomHandle,
    old_is_empty: bool,
    new_is_empty: bool,
}

impl DomCharacterDataMutation {
    pub fn target(&self) -> DomHandle {
        self.target
    }

    pub fn emptiness_changed(&self) -> bool {
        self.old_is_empty != self.new_is_empty
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomAttributeMutation {
    pub(super) target: DomHandle,
    pub(super) local_name: Arc<str>,
    pub(super) namespace: Option<Arc<str>>,
    pub(super) old_value: Option<Arc<str>>,
    pub(super) new_value: Option<Arc<str>>,
}

impl DomAttributeMutation {
    pub(super) fn new(
        target: DomHandle,
        local_name: &str,
        namespace: Option<&str>,
        old_value: Option<Arc<str>>,
        new_value: Option<&str>,
    ) -> Self {
        Self {
            target,
            local_name: Arc::from(local_name),
            namespace: namespace.map(Arc::from),
            old_value,
            new_value: new_value.map(Arc::from),
        }
    }

    pub fn target(&self) -> DomHandle {
        self.target
    }

    pub fn local_name(&self) -> &str {
        &self.local_name
    }

    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    pub fn old_value(&self) -> Option<&str> {
        self.old_value.as_deref()
    }

    pub fn new_value(&self) -> Option<&str> {
        self.new_value.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomChildListMutation {
    pub(super) target: DomHandle,
    pub(super) added_nodes: Arc<[DomHandle]>,
    pub(super) removed_nodes: Arc<[DomHandle]>,
    pub(super) previous_sibling: Option<DomHandle>,
    pub(super) next_sibling: Option<DomHandle>,
}

impl DomChildListMutation {
    pub(super) fn new(
        target: DomHandle,
        added_nodes: &[DomHandle],
        removed_nodes: &[DomHandle],
        previous_sibling: Option<DomHandle>,
        next_sibling: Option<DomHandle>,
    ) -> Self {
        Self {
            target,
            added_nodes: Arc::from(added_nodes),
            removed_nodes: Arc::from(removed_nodes),
            previous_sibling,
            next_sibling,
        }
    }

    pub fn target(&self) -> DomHandle {
        self.target
    }

    pub fn added_nodes(&self) -> &[DomHandle] {
        &self.added_nodes
    }

    pub fn shared_added_nodes(&self) -> Arc<[DomHandle]> {
        Arc::clone(&self.added_nodes)
    }

    pub fn removed_nodes(&self) -> &[DomHandle] {
        &self.removed_nodes
    }

    pub fn shared_removed_nodes(&self) -> Arc<[DomHandle]> {
        Arc::clone(&self.removed_nodes)
    }

    pub fn previous_sibling(&self) -> Option<DomHandle> {
        self.previous_sibling
    }

    pub fn next_sibling(&self) -> Option<DomHandle> {
        self.next_sibling
    }
}

impl DomMutationEffects {
    pub(in crate::native::host::mutation) fn mark_style_character_data_mutation(
        &mut self,
        target: DomHandle,
        old_is_empty: bool,
        new_is_empty: bool,
    ) {
        self.changed = true;
        self.style
            .push_character_data_mutation(DomCharacterDataMutation {
                target,
                old_is_empty,
                new_is_empty,
            });
    }

    pub(in crate::native::host::mutation) fn mark_style_child_list_mutation(
        &mut self,
        target: DomHandle,
        added_nodes: &[DomHandle],
        removed_nodes: &[DomHandle],
        previous_sibling: Option<DomHandle>,
        next_sibling: Option<DomHandle>,
    ) {
        if added_nodes.is_empty() && removed_nodes.is_empty() {
            return;
        }
        self.changed = true;
        let mutation = DomChildListMutation::new(
            target,
            added_nodes,
            removed_nodes,
            previous_sibling,
            next_sibling,
        );
        self.push_style_child_list_mutation(mutation);
    }

    pub(super) fn push_style_child_list_mutation(&mut self, mutation: DomChildListMutation) {
        self.style.push_child_list_mutation(mutation);
    }
}

#[cfg(test)]
mod tests {
    use crate::native::{DomHost, NativeDom};

    #[test]
    fn character_data_retains_writes_but_distinguishes_selector_content_transitions() {
        let mut host = DomHost::from_dom(NativeDom::new_html(
            url::Url::parse("https://text.test/").unwrap(),
        ));
        for node in [host.create_text_node("A"), host.create_cdata_section("A")] {
            for (value, emptiness_changed) in [("א", false), ("", true), (" ", true), ("B", false)]
            {
                let effects = host.set_text_content_effects(node, value);
                assert!(effects.did_change());
                let mutations = effects.style().character_data_mutations();
                assert_eq!(mutations.len(), 1);
                assert_eq!(mutations[0].target(), node);
                assert_eq!(mutations[0].emptiness_changed(), emptiness_changed);
                assert!(!host.set_text_content_effects(node, value).did_change());
            }
        }
    }

    #[test]
    fn merged_character_data_keeps_first_and_last_emptiness_without_losing_the_write() {
        let mut host = DomHost::from_dom(NativeDom::new_html(
            url::Url::parse("https://text.test/").unwrap(),
        ));
        let first = host.create_text_node("A");
        let second = host.create_text_node("");
        let mut effects = host.set_text_content_effects(first, "");
        effects.merge(host.set_text_content_effects(second, "B"));
        effects.merge(host.set_text_content_effects(first, "א"));
        let mutations = effects.style().character_data_mutations();
        assert_eq!(mutations.len(), 2);
        assert_eq!(mutations[0].target(), first);
        assert!(!mutations[0].emptiness_changed());
        assert_eq!(mutations[1].target(), second);
        assert!(mutations[1].emptiness_changed());
        effects.merge(host.set_text_content_effects(second, ""));
        assert!(effects.did_change());
        assert!(
            effects
                .style()
                .character_data_mutations()
                .iter()
                .all(|m| !m.emptiness_changed())
        );
    }
}

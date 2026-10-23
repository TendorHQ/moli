use super::*;
use crate::native::{NativeDom, ShadowRootInit};
use HtmlDirection::{Ltr, Rtl};
use std::collections::HashMap;

fn host() -> DomHost {
    DomHost::from_dom(NativeDom::new_html(
        url::Url::parse("https://direction.test/").unwrap(),
    ))
}

fn element(host: &mut DomHost, parent: DomHandle, name: &str, dir: Option<&str>) -> DomHandle {
    let node = host.create_element(name);
    if let Some(dir) = dir {
        assert!(host.set_attribute(node, "dir", dir));
    }
    assert!(host.append_child(parent, node));
    node
}

fn text(host: &mut DomHost, parent: DomHandle, data: &str) -> DomHandle {
    let node = host.create_text_node(data);
    assert!(host.append_child(parent, node));
    node
}

fn assert_changes(
    effects: &DomMutationEffects,
    expected: &[(DomHandle, HtmlDirection, HtmlDirection)],
) {
    let actual = effects
        .directionality_changes()
        .map(|c| (c.target(), (c.before(), c.after())))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        actual,
        expected
            .iter()
            .map(|&(node, before, after)| (node, (before, after)))
            .collect()
    );
}

#[test]
fn text_changes_publish_real_old_values_for_inherited_descendants_only() {
    let mut host = host();
    let document = host.document_handle();
    let root = element(&mut host, document, "div", Some("auto"));
    let source = element(&mut host, root, "span", None);
    let value = text(&mut host, source, "א");
    let tail = element(&mut host, root, "span", None);
    let isolated = element(&mut host, root, "span", Some("rtl"));
    let _isolated_child = element(&mut host, isolated, "span", None);
    let _unrelated = element(&mut host, document, "aside", None);
    assert_changes(&host.set_text_content_effects(value, "אא"), &[]);
    assert_changes(
        &host.set_text_content_effects(value, "A"),
        &[(root, Rtl, Ltr), (source, Rtl, Ltr), (tail, Rtl, Ltr)],
    );
    assert_changes(
        &host.set_text_content_effects(value, "١א"),
        &[(root, Ltr, Rtl), (source, Ltr, Rtl), (tail, Ltr, Rtl)],
    );
}

#[test]
fn excluded_subtrees_do_not_change_an_outer_auto_root() {
    let mut host = host();
    let document = host.document_handle();
    let root = element(&mut host, document, "div", Some("auto"));
    text(&mut host, root, "A");
    for tag in ["script", "style", "textarea"] {
        let excluded = element(&mut host, root, tag, None);
        let value = text(&mut host, excluded, "א");
        assert_changes(&host.set_text_content_effects(value, "A"), &[]);
    }
    let bdi = element(&mut host, root, "bdi", None);
    let value = text(&mut host, bdi, "א");
    assert_changes(
        &host.set_text_content_effects(value, "A"),
        &[(bdi, Rtl, Ltr)],
    );
}

#[test]
fn changing_an_exclusion_boundary_updates_the_auto_ancestor() {
    let mut host = host();
    let document = host.document_handle();
    let root = element(&mut host, document, "div", Some("auto"));
    let source = element(&mut host, root, "span", Some("rtl"));
    text(&mut host, source, "א");
    let tail = element(&mut host, root, "span", None);
    assert_changes(
        &host.remove_attribute_effects(source, "dir"),
        &[(root, Ltr, Rtl), (tail, Ltr, Rtl)],
    );
    assert_changes(
        &host.set_attribute_effects(source, "dir", "ltr"),
        &[(source, Rtl, Ltr), (root, Rtl, Ltr), (tail, Rtl, Ltr)],
    );
    assert_changes(
        &host.set_attribute_ns_effects(
            source,
            Some("urn:unrelated"),
            Some("x"),
            "dir",
            "x:dir",
            "rtl",
        ),
        &[],
    );
}

#[test]
fn compound_mutations_coalesce_first_and_last_direction() {
    let mut host = host();
    let document = host.document_handle();
    let root = element(&mut host, document, "div", Some("auto"));
    text(&mut host, root, "א");
    assert_changes(&host.set_text_content_effects(root, "אב"), &[]);
    let mut effects = host.set_text_content_effects(root, "A");
    assert_changes(&effects, &[(root, Rtl, Ltr)]);
    effects.merge(host.set_text_content_effects(root, "א"));
    assert_changes(&effects, &[]);
    effects.merge(host.set_text_content_effects(root, "B"));
    assert_changes(&effects, &[(root, Rtl, Ltr)]);
}

#[test]
fn tree_moves_include_shadow_inheritance_and_slot_host_dependencies() {
    let mut host = host();
    let document = host.document_handle();
    let left = element(&mut host, document, "div", Some("rtl"));
    let right = element(&mut host, document, "div", Some("ltr"));
    let moved = element(&mut host, left, "section", None);
    let child = element(&mut host, moved, "span", None);
    let shadow = host.attach_shadow_root(moved, "open").unwrap();
    let auto = element(&mut host, shadow, "div", Some("auto"));
    let slot = element(&mut host, auto, "slot", None);
    let isolated = element(&mut host, shadow, "span", Some("rtl"));
    element(&mut host, isolated, "span", None);
    assert_changes(
        &host.append_child_effects(right, moved),
        &[
            (moved, Rtl, Ltr),
            (child, Rtl, Ltr),
            (auto, Rtl, Ltr),
            (slot, Rtl, Ltr),
        ],
    );
}

#[test]
fn auto_slot_changes_do_not_follow_css_inheritance_to_assigned_elements() {
    let mut host = host();
    let document = host.document_handle();
    let owner = element(&mut host, document, "div", None);
    let shadow = host.attach_shadow_root(owner, "open").unwrap();
    let slot = element(&mut host, shadow, "slot", Some("auto"));
    let fallback = element(&mut host, slot, "span", None);
    text(&mut host, fallback, "A");
    let assigned = element(&mut host, owner, "span", None);
    let value = text(&mut host, assigned, "א");
    assert_changes(
        &host.set_text_content_effects(value, "A"),
        &[(slot, Rtl, Ltr), (fallback, Rtl, Ltr)],
    );
    assert_changes(
        &host.set_text_content_effects(value, "א"),
        &[(slot, Ltr, Rtl), (fallback, Ltr, Rtl)],
    );
    assert_changes(
        &host.set_attribute_effects(assigned, "dir", "ltr"),
        &[(slot, Rtl, Ltr), (fallback, Rtl, Ltr)],
    );
}

#[test]
fn manual_reassignment_updates_both_auto_slots_and_query_identity() {
    let mut host = host();
    let document = host.document_handle();
    let owner = element(&mut host, document, "div", None);
    let mut init = ShadowRootInit::new("open");
    init.set_slot_assignment("manual");
    let shadow = host.attach_shadow_root_with_init(owner, init).unwrap();
    let first = element(&mut host, shadow, "slot", Some("auto"));
    let second = element(&mut host, shadow, "slot", Some("auto"));
    let assigned = element(&mut host, owner, "span", None);
    text(&mut host, assigned, "א");
    assert_changes(
        &host.assign_nodes_to_slot(first, vec![assigned]),
        &[(first, Ltr, Rtl)],
    );
    let version = host.query_version();
    assert_changes(
        &host.assign_nodes_to_slot(second, vec![assigned]),
        &[(first, Rtl, Ltr), (second, Ltr, Rtl)],
    );
    assert!(host.query_version() > version);
    let version = host.query_version();
    assert_changes(&host.assign_nodes_to_slot(second, vec![assigned]), &[]);
    assert_eq!(host.query_version(), version);
}

#[test]
fn foreign_slot_attribute_changes_still_invalidate_html_auto_slots() {
    let mut host = host();
    let document = host.document_handle();
    let owner = element(&mut host, document, "div", None);
    let shadow = host.attach_shadow_root(owner, "open").unwrap();
    let slot = element(&mut host, shadow, "slot", Some("auto"));
    let foreign = host
        .create_element_ns(Some("http://www.w3.org/2000/svg"), "text")
        .unwrap();
    text(&mut host, foreign, "א");
    assert!(host.append_child(owner, foreign));
    assert_changes(
        &host.set_attribute_effects(foreign, "slot", "absent"),
        &[(slot, Rtl, Ltr)],
    );
    assert_changes(
        &host.remove_attribute_effects(foreign, "slot"),
        &[(slot, Ltr, Rtl)],
    );
}

#[test]
fn named_slot_insertion_removal_and_renaming_update_the_first_matching_slot() {
    let mut host = host();
    let document = host.document_handle();
    let owner = element(&mut host, document, "div", None);
    let shadow = host.attach_shadow_root(owner, "open").unwrap();
    let first = element(&mut host, shadow, "slot", Some("auto"));
    let assigned = element(&mut host, owner, "span", None);
    text(&mut host, assigned, "א");
    let second = host.create_element("slot");
    host.set_attribute(second, "dir", "auto");
    assert_changes(
        &host.insert_before_effects(shadow, second, Some(first)),
        &[(first, Rtl, Ltr), (second, Ltr, Rtl)],
    );
    assert_changes(
        &host.set_attribute_effects(second, "name", "other"),
        &[(first, Ltr, Rtl), (second, Rtl, Ltr)],
    );
    assert_changes(
        &host.set_attribute_effects(assigned, "slot", "other"),
        &[(first, Rtl, Ltr), (second, Ltr, Rtl)],
    );
    assert_changes(
        &host.remove_child_effects(shadow, second),
        &[(second, Rtl, Ltr)],
    );
    assert_changes(
        &host.remove_attribute_effects(assigned, "slot"),
        &[(first, Ltr, Rtl)],
    );
}

#[test]
fn prebuilt_and_imported_direction_sources_enable_native_invalidation() {
    let mut foreign = host();
    let document = foreign.document_handle();
    let root = element(&mut foreign, document, "div", Some("auto"));
    text(&mut foreign, root, "א");
    let mut prebuilt = DomHost::from_dom(foreign.dom().clone());
    assert!(prebuilt.directionality_may_vary.get());
    assert_changes(
        &prebuilt.set_text_content_effects(root, "A"),
        &[(root, Rtl, Ltr)],
    );

    let mut imported = host();
    let document = imported.document_handle();
    let clone = imported
        .import_foreign_node(document, foreign.dom(), root, true)
        .unwrap();
    assert!(imported.append_child(document, clone));
    assert!(imported.directionality_may_vary.get());
    assert_changes(
        &imported.set_text_content_effects(clone, "A"),
        &[(clone, Rtl, Ltr)],
    );
}

#[test]
fn input_value_and_textarea_dirty_value_have_native_direction_transitions() {
    let mut host = host();
    let document = host.document_handle();
    let input = element(&mut host, document, "input", Some("auto"));
    assert_changes(
        &host.set_input_value_effects(input, "א"),
        &[(input, Ltr, Rtl)],
    );
    assert_changes(
        &host.set_input_value_with_dirty_effects(input, "A", true),
        &[(input, Rtl, Ltr)],
    );
    let textarea = element(&mut host, document, "textarea", Some("auto"));
    text(&mut host, textarea, "א");
    assert_changes(
        &host.set_input_value_from_user_edit_effects(textarea, "A"),
        &[(textarea, Rtl, Ltr)],
    );
    assert_changes(&host.set_text_content_effects(textarea, "אב"), &[]);
}

#[test]
fn plain_ltr_trees_need_no_directionality_snapshot() {
    let mut host = host();
    let document = host.document_handle();
    let root = element(&mut host, document, "div", None);
    let child = element(&mut host, root, "span", None);
    let value = text(&mut host, child, "א");
    assert!(!host.directionality_may_vary.get());
    assert!(
        DirectionalitySnapshot::for_text(&host, value)
            .values
            .is_empty()
    );
    assert_changes(&host.set_text_content_effects(value, "A"), &[]);
    // An implicit bdi auto root must enable transitions, including while detached.
    let bdi = host.create_element("bdi");
    assert_changes(&host.set_text_content_effects(bdi, "א"), &[(bdi, Ltr, Rtl)]);
    assert!(host.directionality_may_vary.get());
}

#[test]
fn parser_attribute_repair_enables_tracking_without_another_tree_insertion() {
    let mut host = host();
    let document = host.document_handle();
    let body = element(&mut host, document, "body", None);
    let source = text(&mut host, body, "א");
    assert!(!host.directionality_may_vary.get());
    let effects = host.add_attrs_if_missing_for_parser(
        body,
        vec![crate::native::Attribute::new(
            "dir".to_owned(),
            String::new(),
            None,
            "auto".to_owned(),
        )],
    );
    assert_changes(&effects, &[(body, Ltr, Rtl)]);
    assert!(host.directionality_may_vary.get());
    assert_changes(
        &host.set_text_content_effects(source, "A"),
        &[(body, Rtl, Ltr)],
    );
}

#[test]
fn parser_text_splices_publish_effects_only_for_connected_nodes() {
    let mut host = host();
    let document = host.document_handle();
    let root = element(&mut host, document, "div", Some("auto"));
    let source = text(&mut host, root, " ");
    assert_changes(
        &host.append_text_for_parser(source, "א"),
        &[(root, Ltr, Rtl)],
    );
    assert_changes(
        &host.prepend_text_for_parser(source, "A".into()),
        &[(root, Rtl, Ltr)],
    );
    assert!(!host.append_text_for_parser(source, "").did_change());
    let fragment = host.create_document_fragment();
    let detached = text(&mut host, fragment, " ");
    assert!(!host.append_text_for_parser(detached, "א").did_change());
    assert!(
        !host
            .prepend_text_for_parser(detached, "A".into())
            .did_change()
    );
    assert_eq!(host.text_content(detached).as_deref(), Some("A א"));
}

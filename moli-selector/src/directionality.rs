use crate::{HtmlDirection, first_strong_text_direction, html_directionality};
use moli_dom::NodeId;
use moli_dom::native::DomHost;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{QueryEngine, dom::native::NativeDom};

    #[test]
    fn first_strong_direction_uses_unicode_bidi_classes() {
        for (text, expected) in [
            ("", None),
            ("123 .!?", None),
            ("\u{0661}\u{0662}\u{0663}", None),
            ("\u{05b0}\u{064b}", None),
            ("\u{0661}A", Some(HtmlDirection::Ltr)),
            ("\u{05b0}A", Some(HtmlDirection::Ltr)),
            ("\u{064b}\u{05d0}", Some(HtmlDirection::Rtl)),
            ("\u{200e}\u{05d0}", Some(HtmlDirection::Ltr)),
            ("\u{200f}A", Some(HtmlDirection::Rtl)),
            ("\u{061c}A", Some(HtmlDirection::Rtl)),
            ("\u{202b}A\u{202c}", Some(HtmlDirection::Ltr)),
            ("\u{1e900}A", Some(HtmlDirection::Rtl)),
            ("123 \u{05d0}A", Some(HtmlDirection::Rtl)),
            ("123 A\u{05d0}", Some(HtmlDirection::Ltr)),
        ] {
            assert_eq!(first_strong_text_direction(text), expected, "{text:?}");
        }
    }

    fn host() -> DomHost {
        let mut host = DomHost::from_dom(NativeDom::new_html(
            url::Url::parse("https://example.test/").unwrap(),
        ));
        host.reset_html_document_shell();
        host
    }

    fn assert_direction(host: &DomHost, node: NodeId, expected: HtmlDirection) {
        assert_eq!(html_directionality(host, node), expected);
        for direction in [HtmlDirection::Ltr, HtmlDirection::Rtl] {
            assert_eq!(
                QueryEngine
                    .matches_host(host, node, &format!(":dir({})", direction.as_str()))
                    .unwrap(),
                direction == expected,
            );
        }
    }

    #[test]
    fn auto_ignores_isolated_html_subtrees_but_not_foreign_namesakes() {
        let mut host = host();
        for tag in ["script", "style", "textarea", "bdi", "span"] {
            for namespace in ["http://www.w3.org/1999/xhtml", "http://www.w3.org/2000/svg"] {
                let root = host.create_element("div");
                assert!(host.set_attribute(root, "dir", "auto"));
                let excluded = host.create_element_ns(Some(namespace), tag).unwrap();
                if tag == "span" {
                    assert!(host.set_attribute(excluded, "dir", "ltr"));
                }
                assert!(host.set_text_content(excluded, "\u{05d0}"));
                assert!(host.append_child(root, excluded));
                let text = host.create_text_node("A");
                assert!(host.append_child(root, text));
                assert_direction(
                    &host,
                    root,
                    if namespace.ends_with("xhtml") {
                        HtmlDirection::Ltr
                    } else {
                        HtmlDirection::Rtl
                    },
                );
            }
        }
    }

    #[test]
    fn only_null_namespace_html_dir_attributes_control_direction() {
        let mut host = host();
        let parent = host.create_element("div");
        assert!(host.set_attribute(parent, "dir", "rtl"));
        let svg = host
            .create_element_ns(Some("http://www.w3.org/2000/svg"), "svg")
            .unwrap();
        assert!(host.set_attribute(svg, "dir", "ltr"));
        assert!(host.append_child(parent, svg));
        assert_direction(&host, svg, HtmlDirection::Rtl);
        let child = host.create_element("span");
        assert!(host.set_attribute_ns(child, Some("urn:test"), None, "dir", "ltr"));
        assert!(host.append_child(parent, child));
        assert_direction(&host, child, HtmlDirection::Rtl);
        for value in ["", "unknown", " ltr", "ltr "] {
            assert!(host.set_attribute_ns(child, None, None, "dir", value));
            assert_direction(&host, child, HtmlDirection::Rtl);
        }
        assert!(host.set_attribute_ns(child, None, None, "dir", "LtR"));
        assert_direction(&host, child, HtmlDirection::Ltr);
    }

    #[test]
    fn textarea_direction_uses_live_value_or_direct_default_text() {
        let mut host = host();
        let textarea = host.create_element("textarea");
        assert!(host.set_attribute(textarea, "dir", "auto"));
        assert!(host.set_text_content(textarea, "\u{05d0}"));
        assert_direction(&host, textarea, HtmlDirection::Rtl);
        assert!(host.set_input_value(textarea, "A"));
        assert_direction(&host, textarea, HtmlDirection::Ltr);
        assert!(host.set_input_value(textarea, "\u{1e900}"));
        assert_direction(&host, textarea, HtmlDirection::Rtl);
        assert!(host.set_input_value(textarea, "\u{0661}\u{05b0}"));
        assert_direction(&host, textarea, HtmlDirection::Ltr);
        assert!(host.set_input_value_with_dirty(textarea, "", false));
        assert_direction(&host, textarea, HtmlDirection::Rtl);
        assert!(host.set_text_content(textarea, ""));
        let descendant = host.create_element("span");
        assert!(host.set_text_content(descendant, "\u{05d0}"));
        assert!(host.append_child(textarea, descendant));
        assert_direction(&host, textarea, HtmlDirection::Ltr);
    }

    #[test]
    fn assigned_slot_auto_uses_assigned_nodes_then_fallback_content() {
        let mut host = host();
        let owner = host.create_element("div");
        let shadow = host.attach_shadow_root(owner, "open").unwrap();
        let slot = host.create_element("slot");
        assert!(host.set_attribute(slot, "dir", "auto"));
        assert!(host.set_text_content(slot, "\u{05d0}"));
        assert!(host.append_child(shadow, slot));
        assert_direction(&host, slot, HtmlDirection::Rtl);

        let assigned = host.create_element("span");
        assert!(host.set_text_content(assigned, "A"));
        assert!(host.append_child(owner, assigned));
        assert_direction(&host, slot, HtmlDirection::Ltr);
        assert!(host.set_text_content(assigned, "\u{05d0}"));
        assert_direction(&host, slot, HtmlDirection::Rtl);
        assert!(host.set_attribute(assigned, "dir", "rtl"));
        // A nonempty assignment list of excluded nodes does not expose fallback.
        assert_direction(&host, slot, HtmlDirection::Ltr);
        assert!(host.set_attribute(assigned, "slot", "other"));
        assert_direction(&host, slot, HtmlDirection::Rtl);
        let text = host.create_text_node("A");
        assert!(host.append_child(owner, text));
        assert_direction(&host, slot, HtmlDirection::Ltr);
    }

    #[test]
    fn contained_slot_contributes_host_direction_not_slot_fallback_text() {
        let mut host = host();
        let owner = host.create_element("div");
        assert!(host.set_attribute(owner, "dir", "rtl"));
        let shadow = host.attach_shadow_root(owner, "open").unwrap();
        let container = host.create_element("div");
        assert!(host.set_attribute(container, "dir", "auto"));
        assert!(host.append_child(shadow, container));
        let slot = host.create_element("slot");
        assert!(host.set_text_content(slot, "A"));
        assert!(host.append_child(container, slot));
        assert_direction(&host, container, HtmlDirection::Rtl);
        assert!(host.set_attribute(owner, "dir", "ltr"));
        assert!(host.set_text_content(slot, "\u{05d0}"));
        assert_direction(&host, container, HtmlDirection::Ltr);
        assert!(host.set_attribute(slot, "dir", "rtl"));
        let text = host.create_text_node("\u{05d0}");
        assert!(host.append_child(container, text));
        assert_direction(&host, container, HtmlDirection::Rtl);
    }

    #[test]
    fn direction_inherits_from_dom_parent_not_assigned_slot_or_css() {
        let mut host = host();
        let owner = host.create_element("div");
        assert!(host.set_attribute(owner, "dir", "ltr"));
        assert!(host.set_attribute(owner, "style", "direction:rtl"));
        let shadow = host.attach_shadow_root(owner, "open").unwrap();
        let slot = host.create_element("slot");
        assert!(host.set_attribute(slot, "dir", "rtl"));
        assert!(host.append_child(shadow, slot));
        let assigned = host.create_element("span");
        assert!(host.append_child(owner, assigned));
        assert_direction(&host, assigned, HtmlDirection::Ltr);
        let shadow_child = host.create_element("span");
        assert!(host.append_child(shadow, shadow_child));
        assert_direction(&host, shadow_child, HtmlDirection::Ltr);
        assert!(host.set_attribute(owner, "dir", "rtl"));
        assert_direction(&host, assigned, HtmlDirection::Rtl);
        assert_direction(&host, shadow_child, HtmlDirection::Rtl);
    }

    #[test]
    fn bdi_and_auto_form_controls_have_their_own_direction_source() {
        let mut host = host();
        let parent = host.create_element("div");
        assert!(host.set_attribute(parent, "dir", "rtl"));
        let bdi = host.create_element("bdi");
        assert!(host.append_child(parent, bdi));
        assert_direction(&host, bdi, HtmlDirection::Ltr);
        assert!(host.set_attribute(bdi, "dir", "invalid"));
        assert!(host.set_text_content(bdi, "\u{200f}A"));
        assert_direction(&host, bdi, HtmlDirection::Rtl);
        assert!(host.set_attribute(bdi, "dir", "ltr"));
        assert_direction(&host, bdi, HtmlDirection::Ltr);

        for kind in [
            "hidden", "text", "search", "tel", "url", "email", "password", "submit", "reset",
            "button",
        ] {
            let input = host.create_element("input");
            assert!(host.set_attribute(input, "type", kind));
            assert!(host.append_child(parent, input));
            assert_direction(
                &host,
                input,
                if kind == "tel" {
                    HtmlDirection::Ltr
                } else {
                    HtmlDirection::Rtl
                },
            );
            assert!(host.set_attribute(input, "dir", "auto"));
            assert!(host.set_input_value(input, "\u{200f}A"));
            assert_direction(&host, input, HtmlDirection::Rtl);
            assert!(host.set_input_value(input, "\u{0661}A"));
            assert_direction(&host, input, HtmlDirection::Ltr);
        }
        let number = host.create_element("input");
        assert!(host.set_attribute(number, "type", "number"));
        assert!(host.set_attribute(number, "dir", "auto"));
        assert!(host.append_child(parent, number));
        assert!(host.set_input_value(number, "123"));
        assert_direction(&host, number, HtmlDirection::Ltr);
    }

    #[test]
    fn nested_shadow_host_direction_dependencies_are_resolved_iteratively() {
        let mut host = host();
        let root = host.create_element("div");
        assert!(host.set_attribute(root, "dir", "rtl"));
        let mut owner = root;
        for _ in 0..512 {
            let shadow = host.attach_shadow_root(owner, "open").unwrap();
            let next = host.create_element("div");
            assert!(host.set_attribute(next, "dir", "auto"));
            assert!(host.append_child(shadow, next));
            let slot = host.create_element("slot");
            assert!(host.set_text_content(slot, "A"));
            assert!(host.append_child(next, slot));
            owner = next;
        }
        assert_direction(&host, owner, HtmlDirection::Rtl);
        assert!(host.set_attribute(root, "dir", "ltr"));
        assert_direction(&host, owner, HtmlDirection::Ltr);
    }
}

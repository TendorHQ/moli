use crate::{Dom, NodeKind};

pub(crate) fn fallback_text<D: Dom + ?Sized>(dom: &D, node: D::NodeId) -> Option<String> {
    if dom
        .attribute(node, "aria-hidden")
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
        || dom.attribute(node, "hidden").is_some()
        || dom.attribute(node, "role").is_some_and(|role| {
            matches!(
                role.split_ascii_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .as_str(),
                "none" | "presentation"
            )
        })
    {
        return None;
    }
    let named_tag = matches!(
        dom.node_kind(node),
        NodeKind::Element(
            "a" | "button"
                | "img"
                | "svg"
                | "input"
                | "textarea"
                | "select"
                | "option"
                | "optgroup"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
        )
    );
    let named_role = dom.attribute(node, "role").is_some_and(|role| {
        matches!(
            role.split_ascii_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "button"
                | "link"
                | "img"
                | "heading"
                | "checkbox"
                | "radio"
                | "textbox"
                | "combobox"
                | "switch"
                | "tab"
        )
    });
    if !named_tag && !named_role {
        return None;
    }
    for name in ["aria-label", "title"] {
        if let Some(value) = dom
            .attribute(node, name)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_owned());
        }
    }
    if matches!(dom.node_kind(node), NodeKind::Element("svg")) {
        let mut child = dom.first_child(node);
        while let Some(id) = child {
            if dom.node_kind(id) == NodeKind::Element("title") {
                let mut text = String::new();
                let mut title_child = dom.first_child(id);
                while let Some(part) = title_child {
                    if let NodeKind::Text(value) = dom.node_kind(part) {
                        text.push_str(value);
                    }
                    title_child = dom.next_sibling(part);
                }
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
            child = dom.next_sibling(id);
        }
    }
    None
}

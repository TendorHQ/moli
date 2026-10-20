mod support;

use moli_html2md::convert;
use std::cell::Cell;
use support::{Tree, rendered_html};

struct Counted {
    tree: Tree,
    visits: Cell<usize>,
}
impl moli_html2md::Dom for Counted {
    type NodeId = usize;
    fn node_kind(&self, node: usize) -> moli_html2md::NodeKind<'_> {
        self.visits.set(self.visits.get() + 1);
        self.tree.node_kind(node)
    }
    fn first_child(&self, node: usize) -> Option<usize> {
        self.tree.first_child(node)
    }
    fn next_sibling(&self, node: usize) -> Option<usize> {
        self.tree.next_sibling(node)
    }
    fn attribute(&self, node: usize, name: &str) -> Option<&str> {
        self.tree.attribute(node, name)
    }
}

fn markdown(html: &str) -> String {
    let tree = Tree::parse(html);
    convert(&tree, tree.root)
}

#[test]
fn private_class_names_do_not_invent_document_semantics() {
    let result = markdown(
        "<div class='quote'>Request a price quote</div><i class='fa-check'></i>\
         <blockquote>Actual quotation</blockquote>\
         <span data-toggle='tooltip' data-original-title='Private plugin state'></span>\
         <button aria-label='Save'></button>",
    );
    let html = rendered_html(&result);
    assert!(!result.contains('✓'), "{result}");
    assert!(!result.contains("Private plugin state"), "{result}");
    assert!(html.contains("<p>Request a price quote</p>"), "{html}");
    assert!(html.contains("<blockquote>"), "{html}");
    assert!(result.contains("Actual quotation"), "{result}");
    assert!(result.contains("Save"), "{result}");
}

#[test]
fn standard_image_sources_are_not_overridden_by_filename_guesses() {
    for source in [
        "/photos/placeholder.gif",
        "/art/blank.png",
        "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==",
    ] {
        let result = markdown(&format!(
            "<img src='{source}' data-src='/private-alternative.jpg' alt='Original'>"
        ));
        assert!(result.contains(source), "{result}");
        assert!(!result.contains("private-alternative"), "{result}");
    }
    let result =
        markdown("<img src='javascript:invalid()' data-src='/declared-photo.jpg' alt='Fallback'>");
    assert!(result.contains("declared-photo.jpg"), "{result}");
}

#[test]
fn nested_accessible_labels_do_not_rescan_the_whole_tree() {
    let count = |depth| {
        let html = format!(
            "{}Readable terminal{}",
            "<span aria-label='Fallback'>".repeat(depth),
            "</span>".repeat(depth),
        );
        let dom = Counted {
            tree: Tree::parse(&html),
            visits: Cell::new(0),
        };
        assert_eq!(convert(&dom, dom.tree.root), "Readable terminal");
        dom.visits.get()
    };
    let small = count(100);
    let large = count(400);
    assert!(large <= small * 5, "{small} -> {large}");
}

#[test]
fn textarea_uses_its_content_not_a_nonstandard_value_attribute() {
    for value in ["unrelated", ""] {
        let result = markdown(&format!(
            "<div title='Group'><textarea value='{value}'>Authored text</textarea></div>"
        ));
        assert!(result.contains("Authored text"), "{result}");
        assert!(!result.contains("unrelated"), "{result}");
    }
}

#[test]
fn accessible_label_queries_remain_linear_with_large_unreadable_branches() {
    let count = |depth| {
        let branch = "<svg><path d='M0 0'/><path d='M1 1'/></svg>".repeat(20);
        let html = format!(
            "{}{}{}",
            format!("<span aria-label='Fallback'>{branch}").repeat(depth),
            branch,
            "</span>".repeat(depth)
        );
        let dom = Counted {
            tree: Tree::parse(&html),
            visits: Cell::new(0),
        };
        let result = convert(&dom, dom.tree.root);
        assert_eq!(result.matches("Fallback").count(), 1, "{result}");
        dom.visits.get()
    };
    let small = count(30);
    let large = count(120);
    assert!(large <= small * 5, "{small} -> {large}");
}

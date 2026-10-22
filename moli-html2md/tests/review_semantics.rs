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
            "<span role='button' aria-label='Fallback'>".repeat(depth),
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
            format!("<span role='button' aria-label='Fallback'>{branch}").repeat(depth),
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

#[test]
fn labels_cannot_replace_media_choices_or_rules() {
    for html in [
        "<div title='Demo'><video src='/demo.mp4'></video></div>",
        "<a href='/watch' title='Watch'><iframe src='https://example.test/movie'></iframe></a>",
        "<button aria-label='Demo'><audio><source src='/demo.ogg'></audio></button>",
    ] {
        let result = markdown(html);
        assert!(
            result.contains("demo.mp4")
                || result.contains("example.test/movie")
                || result.contains("demo.ogg"),
            "{html}: {result}"
        );
    }
    let result = markdown(
        "<button aria-label='Choices'><select><optgroup label='Group'><option label='First'></option></optgroup></select></button>",
    );
    assert!(
        result.contains("Group") && result.contains("First"),
        "{result}"
    );
    let result = markdown("<button aria-label='Rule'><hr></button>");
    assert!(result.contains("---"), "{result}");
}

#[test]
fn accessible_fallback_is_named_content_and_has_word_boundaries() {
    let result = markdown(
        "<p>x<button aria-label='Save'></button>y</p><nav aria-label='Pagination'></nav><div aria-label='Advertisement'></div><span title='tooltip'></span><button aria-hidden='true' aria-label='Hidden'></button><div role='progressbar' aria-label='Loading'></div><button aria-label='Download'><script type='application/json'>private state</script></button>",
    );
    assert!(result.contains("x Save y"), "{result}");
    assert!(result.contains("Download"), "{result}");
    for excluded in [
        "Pagination",
        "Advertisement",
        "tooltip",
        "Hidden",
        "Loading",
        "private state",
    ] {
        assert!(!result.contains(excluded), "{result}");
    }
}

#[test]
fn phrasing_controls_and_dates_keep_inline_styles_and_table_cells() {
    let result = markdown(
        "<strong>Posted <time>today</time></strong><a href='/p'>Posted <time>Oct 1</time></a><table><tr><th>Date</th><th>Action</th></tr><tr><td>x <time>Oct 1</time> y</td><td>a <button>Save</button> b</td></tr></table>",
    );
    let html = rendered_html(&result);
    assert!(html.contains("<strong>Posted today</strong>"), "{html}");
    assert!(html.contains("<a href=\"/p\">Posted Oct 1</a>"), "{html}");
    assert!(!result.contains("<br>"), "{result}");
}

#[test]
fn input_hints_do_not_masquerade_as_filled_values() {
    let result = markdown(
        "<input type='email' value='' placeholder='Your Email'><input value='Actual value'><input type='password' value='secret' aria-label='Password'>",
    );
    assert!(!result.contains("Your Email"), "{result}");
    assert!(
        result.contains("Actual value") && result.contains("Password"),
        "{result}"
    );
    assert!(!result.contains("secret"), "{result}");
}

#[test]
fn links_retain_legitimate_non_web_destinations() {
    for href in [
        "magnet:?xt=urn:btih:123",
        "sms:+12025550123",
        "geo:1,2",
        "file:///tmp/manual.pdf",
        "myapp:open/123",
        "blob:https://example.test/id",
        "data:application/pdf;base64,JVBERi0=",
    ] {
        let result = markdown(&format!("<a href='{href}'>Open</a>"));
        assert!(result.contains(href), "{href}: {result}");
    }
    for href in [
        "javascript:alert(1)",
        "vbscript:run()",
        "data:text/html,unsafe",
    ] {
        let result = markdown(&format!("<a href='{href}'>Safe label</a>"));
        assert!(!result.contains(href), "{result}");
    }
}

//! Tagged-PDF logical structure tree (/StructTreeRoot).

use pdfkit_core::{Document, Engine, OpenOptions};

fn open(bytes: Vec<u8>) -> Document {
    Engine::new()
        .unwrap()
        .open(bytes, OpenOptions::default())
        .expect("open")
}

#[test]
fn structure_tree_recovers_tags_text_alt_and_reading_order() {
    let doc = open(pdfkit_fixtures::tagged_minimal());
    let root = doc.structure_tree().expect("a tagged document");
    assert_eq!(root.tag, "Root");

    // Document element wraps the three content elements.
    assert_eq!(root.children.len(), 1);
    let document = &root.children[0];
    assert_eq!(document.tag, "Document");
    assert_eq!(document.children.len(), 3);

    let (h1, p, fig) = (
        &document.children[0],
        &document.children[1],
        &document.children[2],
    );

    // Tags and per-element text recovered from marked content, in reading order.
    assert_eq!(h1.tag, "H1");
    assert_eq!(h1.text, "Title");
    assert_eq!(h1.page, Some(1));

    assert_eq!(p.tag, "P");
    assert_eq!(p.text, "Paragraph.");

    // Figure alt-text recovered.
    assert_eq!(fig.tag, "Figure");
    assert_eq!(fig.text, "Figure");
    assert_eq!(fig.alt.as_deref(), Some("A pie chart"));
}

#[test]
fn untagged_document_has_no_structure_tree() {
    let doc = open(pdfkit_fixtures::born_digital());
    assert!(doc.structure_tree().is_none());
}

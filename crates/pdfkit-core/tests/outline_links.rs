//! Outline (bookmarks), link annotations, and extended info-dict metadata.

use pdfkit_core::{Document, Engine, LinkTarget, OpenOptions};

fn open(bytes: Vec<u8>) -> Document {
    Engine::new()
        .unwrap()
        .open(bytes, OpenOptions::default())
        .expect("open")
}

#[test]
fn outline_is_a_tree_with_resolved_pages() {
    let doc = open(pdfkit_fixtures::outline_and_links());
    let outline = doc.outline();
    assert_eq!(outline.len(), 2, "two top-level bookmarks");

    assert_eq!(outline[0].title, "Chapter 1");
    assert_eq!(outline[0].page, Some(1));
    assert_eq!(outline[0].children.len(), 1);
    assert_eq!(outline[0].children[0].title, "Section 1.1");
    assert_eq!(outline[0].children[0].page, Some(2));

    assert_eq!(outline[1].title, "Chapter 2");
    assert_eq!(outline[1].page, Some(2));
    assert!(outline[1].children.is_empty());
}

#[test]
fn links_resolve_external_and_internal_targets() {
    let doc = open(pdfkit_fixtures::outline_and_links());
    let links = doc.page(1).expect("page 1").links();
    assert_eq!(links.len(), 2);

    let uri = links
        .iter()
        .find(|l| matches!(&l.target, LinkTarget::Uri(_)))
        .expect("a URI link");
    assert_eq!(uri.target, LinkTarget::Uri("https://example.com".into()));
    // Rect is normalized [x0,y0,x1,y1].
    assert_eq!(uri.rect, [50.0, 700.0, 150.0, 720.0]);

    assert!(
        links.iter().any(|l| l.target == LinkTarget::Page(2)),
        "an internal link to page 2"
    );
}

#[test]
fn extended_metadata_is_read() {
    let doc = open(pdfkit_fixtures::outline_and_links());
    let m = doc.metadata();
    assert_eq!(m.title.as_deref(), Some("Outline and Link Fixture"));
    assert_eq!(m.subject.as_deref(), Some("outline + link test"));
    assert_eq!(m.keywords.as_deref(), Some("outlines, links"));
    assert_eq!(m.creator.as_deref(), Some("pdfkit-fixtures"));
    assert_eq!(m.producer.as_deref(), Some("lopdf"));
}

#[test]
fn cyclic_outline_terminates() {
    let doc = open(pdfkit_fixtures::cyclic_outline());
    let outline = doc.outline(); // must not hang or overflow the stack
    assert_eq!(outline.len(), 1);
    assert_eq!(outline[0].title, "Loop");
    assert_eq!(outline[0].page, Some(1));
    // The self-referential /First is pruned by the cycle guard.
    assert!(outline[0].children.is_empty());
}

#[test]
fn documents_without_outline_or_links_are_empty() {
    let doc = open(pdfkit_fixtures::born_digital());
    assert!(doc.outline().is_empty());
    assert!(doc.page(1).expect("page 1").links().is_empty());
}

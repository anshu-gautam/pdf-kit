//! Chunking from the tagged structure tree, and figures as chunks.

use pdfkit_chunk::{chunk_document, ChunkOptions, ElementKind};
use pdfkit_core::{Engine, OpenOptions};

fn chunks_of(bytes: Vec<u8>) -> Vec<pdfkit_chunk::Chunk> {
    let doc = Engine::new()
        .unwrap()
        .open(bytes, OpenOptions::default())
        .unwrap();
    chunk_document(&doc, &ChunkOptions::default()).expect("chunk")
}

#[test]
fn tagged_document_chunks_from_the_structure_tree() {
    // tagged_minimal: Document > H1 "Title", P "Paragraph.", Figure (/Alt).
    let chunks = chunks_of(pdfkit_fixtures::tagged_minimal());
    assert_eq!(
        chunks.len(),
        3,
        "{:?}",
        chunks
            .iter()
            .map(|c| (&c.kind, &c.text))
            .collect::<Vec<_>>()
    );

    assert_eq!(chunks[0].kind, ElementKind::Heading);
    assert_eq!(chunks[0].text, "Title");
    assert_eq!(chunks[0].heading_path, Vec::<String>::new());

    assert_eq!(chunks[1].kind, ElementKind::Paragraph);
    assert_eq!(chunks[1].text, "Paragraph.");
    // Authoritative breadcrumb from the H1 tag (not a font-size guess).
    assert_eq!(chunks[1].heading_path, vec!["Title".to_string()]);

    // The Figure's text is its /Alt, not its drawn glyphs.
    assert_eq!(chunks[2].kind, ElementKind::Figure);
    assert_eq!(chunks[2].text, "A pie chart");

    // Tagged chunks now carry a measured bbox derived from their marked-content
    // runs (each MCID is positioned via a Tm in the fixture), plus page.
    assert!(chunks.iter().all(|c| c.page == 1));
    for c in &chunks {
        let bbox = c
            .bbox
            .unwrap_or_else(|| panic!("tagged chunk has a bbox: {c:?}"));
        assert!(
            bbox[0] < bbox[2] && bbox[1] < bbox[3],
            "ordered bbox: {bbox:?}"
        );
    }
}

#[test]
fn untagged_document_emits_figure_chunks() {
    // figure_with_caption is NOT tagged -> geometry path, but its image becomes
    // a Figure chunk carrying the caption, with a real bbox.
    let cap = "Figure 1: A sample chart.";
    let chunks = chunks_of(pdfkit_fixtures::figure_with_caption());
    let figures: Vec<_> = chunks
        .iter()
        .filter(|c| c.kind == ElementKind::Figure)
        .collect();
    assert_eq!(figures.len(), 1, "exactly one figure chunk");
    assert!(figures[0].text.contains(cap));
    let bbox = figures[0].bbox.expect("geometry figure keeps its bbox");
    let near = |a: f32, b: f32| (a - b).abs() < 1.0;
    assert!(
        near(bbox[0], 100.0) && near(bbox[2], 500.0),
        "bbox {bbox:?}"
    );
    // The caption is adopted by the figure, not also emitted as its own chunk.
    assert_eq!(
        chunks.iter().filter(|c| c.text.contains(cap)).count(),
        1,
        "caption must appear exactly once"
    );
}

#[test]
fn untagged_text_only_docs_are_unchanged_no_figures() {
    let chunks = chunks_of(pdfkit_fixtures::multi_heading());
    assert!(chunks.iter().all(|c| c.kind != ElementKind::Figure));
    assert!(chunks.iter().any(|c| c.kind == ElementKind::Heading));
}

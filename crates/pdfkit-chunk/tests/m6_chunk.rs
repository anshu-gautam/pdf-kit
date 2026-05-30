//! M6 acceptance: chunking yields chunks with correct pages, populated
//! heading_path, and sizes near the target.

use pdfkit_chunk::{chunk_document, ChunkOptions, ElementKind};
use pdfkit_core::{Engine, OpenOptions};

fn chunks_of(bytes: Vec<u8>, opts: &ChunkOptions) -> Vec<pdfkit_chunk::Chunk> {
    let doc = Engine::new()
        .unwrap()
        .open(bytes, OpenOptions::default())
        .unwrap();
    chunk_document(&doc, opts).expect("chunk")
}

#[test]
fn produces_chunks() {
    let chunks = chunks_of(pdfkit_fixtures::multi_heading(), &ChunkOptions::default());
    assert!(!chunks.is_empty());
    // Headings and body are both represented.
    assert!(chunks.iter().any(|c| c.kind == ElementKind::Heading));
    assert!(chunks.iter().any(|c| c.kind == ElementKind::Paragraph));
}

#[test]
fn headings_and_paths_are_correct() {
    let chunks = chunks_of(pdfkit_fixtures::multi_heading(), &ChunkOptions::default());

    // All chunks are on page 1 of this single-page fixture.
    assert!(chunks.iter().all(|c| c.page == 1));

    // Headings present with correct breadcrumbs.
    let chapter_one = chunks
        .iter()
        .find(|c| c.kind == ElementKind::Heading && c.text.contains("Chapter One"))
        .expect("Chapter One heading");
    assert_eq!(chapter_one.heading_path, Vec::<String>::new());

    let section_a = chunks
        .iter()
        .find(|c| c.kind == ElementKind::Heading && c.text.contains("Section A"))
        .expect("Section A heading");
    assert_eq!(section_a.heading_path, vec!["Chapter One".to_string()]);

    // Body under Section A carries the full breadcrumb.
    let alpha = chunks
        .iter()
        .find(|c| c.text.contains("Alpha body"))
        .expect("alpha body chunk");
    assert_eq!(
        alpha.heading_path,
        vec!["Chapter One".to_string(), "Section A".to_string()]
    );
    assert_eq!(alpha.kind, ElementKind::Paragraph);

    // Body under Chapter Two resets to just that chapter.
    let gamma = chunks
        .iter()
        .find(|c| c.text.contains("Gamma body"))
        .expect("gamma body chunk");
    assert_eq!(gamma.heading_path, vec!["Chapter Two".to_string()]);
}

#[test]
fn separate_blocks_pack_together_under_target() {
    let chunks = chunks_of(pdfkit_fixtures::multi_heading(), &ChunkOptions::default());
    // Gamma and Delta are *separate blocks* (a spacer splits them) under the same
    // heading; with a generous target the packer combines them into one chunk.
    let combined = chunks
        .iter()
        .find(|c| c.text.contains("Gamma body"))
        .expect("gamma chunk");
    assert!(
        combined.text.contains("Delta paragraph"),
        "separate blocks should pack under target: {:?}",
        combined.text
    );

    // No chunk exceeds the target token budget.
    for c in &chunks {
        assert!(c.token_estimate <= ChunkOptions::default().target_tokens);
    }
}

#[test]
fn small_target_keeps_blocks_separate() {
    let opts = ChunkOptions {
        target_tokens: 8,
        ..Default::default()
    };
    let chunks = chunks_of(pdfkit_fixtures::multi_heading(), &opts);
    // With a tiny target, Gamma and Delta land in different chunks.
    let gamma = chunks
        .iter()
        .find(|c| c.text.contains("Gamma body"))
        .expect("gamma chunk");
    assert!(
        !gamma.text.contains("Delta paragraph"),
        "tiny target should not pack separate blocks: {:?}",
        gamma.text
    );
    let separate = chunks
        .iter()
        .filter(|c| c.text.contains("Gamma body") || c.text.contains("Delta paragraph"))
        .count();
    assert!(separate >= 2);
}

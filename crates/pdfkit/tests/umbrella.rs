//! The umbrella crate re-exports a working read/extract/chunk/edit/adapter API
//! from one crate. Requires the default features (chunk/edit/adapters), so it is
//! compiled out of the minimal (`--no-default-features`) build.
#![cfg(all(feature = "chunk", feature = "edit", feature = "adapters"))]

use pdfkit::{
    chunk_document, extract, to_message_content, ChunkOptions, Engine, ExtractOptions, FontSpec,
    Mode, OpenOptions, PageSize, PdfBuilder,
};

#[test]
fn one_crate_does_it_all() {
    // Extract (read path).
    let result = extract(
        pdfkit_fixtures::born_digital(),
        ExtractOptions {
            mode: Mode::Text,
            ..Default::default()
        },
    )
    .expect("extract");
    assert!(result.text.contains("Hello, pdfkit!"));

    // Chunk.
    let doc = Engine::new()
        .unwrap()
        .open(pdfkit_fixtures::multi_heading(), OpenOptions::default())
        .unwrap();
    let chunks = chunk_document(&doc, &ChunkOptions::default()).expect("chunk");
    assert!(!chunks.is_empty());

    // Edit (write path).
    let mut builder = PdfBuilder::new();
    let page = builder.add_page(PageSize::Letter);
    builder.draw_text(page, "round trip", (72.0, 700.0), FontSpec::default());
    let mut bytes = Vec::new();
    builder.save(&mut bytes).expect("save");
    assert!(bytes.starts_with(b"%PDF"));

    // Adapters.
    let blocks = to_message_content(&result);
    assert!(!blocks.is_empty());
}

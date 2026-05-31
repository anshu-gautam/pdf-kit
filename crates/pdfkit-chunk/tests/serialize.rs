//! Serialization + provenance: char-offset spans, stable ids, Markdown, the
//! contextual prefix, and JSON round-trip.

use pdfkit_chunk::{chunk_document, document_text, to_markdown, Chunk, ChunkOptions, ElementKind};
use pdfkit_core::{Engine, OpenOptions};

fn chunks_of(bytes: Vec<u8>, opts: &ChunkOptions) -> Vec<Chunk> {
    let doc = Engine::new()
        .unwrap()
        .open(bytes, OpenOptions::default())
        .unwrap();
    chunk_document(&doc, opts).expect("chunk")
}

#[test]
fn char_offsets_index_exactly_into_document_text() {
    let chunks = chunks_of(pdfkit_fixtures::multi_heading(), &ChunkOptions::default());
    assert!(!chunks.is_empty());
    let doc = document_text(&chunks);
    let chars: Vec<char> = doc.chars().collect();
    for c in &chunks {
        assert_eq!(c.char_len, c.text.chars().count());
        let slice: String = chars[c.char_start..c.char_start + c.char_len]
            .iter()
            .collect();
        assert_eq!(
            slice, c.text,
            "char span must slice out the chunk text exactly"
        );
    }
}

#[test]
fn stable_ids_are_deterministic_and_nonempty() {
    let a = chunks_of(pdfkit_fixtures::multi_heading(), &ChunkOptions::default());
    let b = chunks_of(pdfkit_fixtures::multi_heading(), &ChunkOptions::default());
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert!(!x.id.is_empty());
        assert_eq!(x.id, y.id, "same content => same id across runs");
    }
    // Different content => different id (headings vs their body).
    let heading = a.iter().find(|c| c.kind == ElementKind::Heading).unwrap();
    let para = a.iter().find(|c| c.kind == ElementKind::Paragraph).unwrap();
    assert_ne!(heading.id, para.id);
}

#[test]
fn markdown_renders_headings_table_and_caption() {
    let md = to_markdown(&chunks_of(
        pdfkit_fixtures::multi_heading(),
        &ChunkOptions::default(),
    ));
    assert!(md.contains("# Chapter One"), "top heading at level 1: {md}");
    assert!(
        md.contains("## Section A"),
        "nested heading at level 2: {md}"
    );
    assert!(md.contains("Alpha body"));

    let tmd = to_markdown(&chunks_of(
        pdfkit_fixtures::table_doc(),
        &ChunkOptions::default(),
    ));
    assert!(
        tmd.contains("| Name | Role | Level |"),
        "table header row: {tmd}"
    );
    assert!(tmd.contains("| --- |"), "table separator row: {tmd}");
    assert!(tmd.contains("*Figure 1"), "caption rendered italic: {tmd}");
}

#[test]
fn contextual_prefix_is_opt_in() {
    let plain = chunks_of(pdfkit_fixtures::multi_heading(), &ChunkOptions::default());
    assert!(plain.iter().all(|c| c.context.is_none()));

    let opts = ChunkOptions {
        contextual_prefix: true,
        ..Default::default()
    };
    let ctx_chunks = chunks_of(pdfkit_fixtures::multi_heading(), &opts);
    let alpha = ctx_chunks
        .iter()
        .find(|c| c.text.contains("Alpha body"))
        .expect("alpha chunk");
    let ctx = alpha.context.as_ref().expect("context populated");
    assert!(
        ctx.contains("Multi Heading Fixture"),
        "title in context: {ctx}"
    );
    assert!(
        ctx.contains("Chapter One") && ctx.contains("Section A"),
        "breadcrumb: {ctx}"
    );
    assert!(ctx.ends_with("(p.1)"), "page in context: {ctx}");
}

#[test]
fn markdown_escapes_leading_markers_in_prose() {
    let para = Chunk {
        id: String::new(),
        text: "# Not a heading\n| not a table".to_string(),
        context: None,
        page: 1,
        bbox: None,
        kind: ElementKind::Paragraph,
        heading_path: Vec::new(),
        char_start: 0,
        char_len: 0,
        token_estimate: 1,
    };
    let md = to_markdown(std::slice::from_ref(&para));
    assert!(md.contains("\\# Not a heading"), "leading # escaped: {md}");
    assert!(md.contains("\\| not a table"), "leading | escaped: {md}");
    // The escaped text must not render as a real heading line.
    assert!(!md.lines().any(|l| l == "# Not a heading"));

    // A genuine List keeps its markers unescaped.
    let list = Chunk {
        kind: ElementKind::List,
        text: "- first\n- second".to_string(),
        ..para.clone()
    };
    let lmd = to_markdown(std::slice::from_ref(&list));
    assert!(lmd.contains("- first"), "list markers preserved: {lmd}");
}

#[cfg(feature = "serde")]
#[test]
fn json_serializes_and_round_trips() {
    let chunks = chunks_of(pdfkit_fixtures::multi_heading(), &ChunkOptions::default());
    let json = pdfkit_chunk::to_json(&chunks).expect("to_json");
    for key in [
        "\"id\"",
        "\"char_start\"",
        "\"char_len\"",
        "\"heading_path\"",
        "\"bbox\"",
    ] {
        assert!(json.contains(key), "json missing {key}: {json}");
    }
    let back: Vec<Chunk> = serde_json::from_str(&json).expect("parse json");
    assert_eq!(back, chunks, "JSON must round-trip losslessly");
}

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

#[test]
fn tagged_table_reconstructs_a_grid_with_spans() {
    let chunks = chunks_of(pdfkit_fixtures::tagged_table());
    let table = chunks
        .iter()
        .find(|c| c.kind == ElementKind::Table)
        .and_then(|c| c.table.as_ref())
        .expect("a Table chunk with a grid");

    assert_eq!(table.columns, 2);
    assert_eq!(table.header_rows, 1, "the all-TH first row is the header");
    assert_eq!(table.rows.len(), 3);

    // Header + data cells in order, from the TR/TH/TD structure.
    assert_eq!(table.rows[0][0].text, "Name");
    assert_eq!(table.rows[0][1].text, "Role");
    assert_eq!(table.rows[1][0].text, "Ada");
    assert_eq!(table.rows[1][1].text, "Eng");

    // The final row's cell spans both columns (/A /ColSpan 2).
    assert_eq!(table.rows[2][0].text, "Note");
    assert_eq!(table.rows[2][0].colspan, 2);
    assert!(
        table.rows[2][1].text.is_empty(),
        "colspan-covered slot is filler"
    );

    // Each real cell carries a measured bbox from its MCID.
    for cell in [&table.rows[0][0], &table.rows[1][1]] {
        assert!(cell.bbox[0] < cell.bbox[2] && cell.bbox[1] < cell.bbox[3]);
    }

    // Serializations reflect the grid + span.
    let html = table.to_html();
    assert!(
        html.contains("<thead><tr><th>Name</th><th>Role</th></tr></thead>"),
        "{html}"
    );
    assert!(html.contains("<td colspan=\"2\">Note</td>"), "{html}");
    let csv = table.to_csv();
    assert!(csv.contains("Name,Role"));
    assert!(
        csv.lines().any(|l| l == "Note,"),
        "spanned cols blank in csv: {csv}"
    );
}

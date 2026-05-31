//! Ruled-line table reconstruction: true colspan/rowspan inferred from the
//! vector grid lines (the case text-gap inference can't recover).

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
fn ruled_table_recovers_a_spanning_header() {
    let chunks = chunks_of(pdfkit_fixtures::ruled_table_spanning_cell());
    let table = chunks
        .iter()
        .find(|c| c.kind == ElementKind::Table)
        .and_then(|c| c.table.as_ref())
        .unwrap_or_else(|| {
            panic!(
                "expected a Table chunk with a grid: {:?}",
                chunks.iter().map(|c| (c.kind, &c.text)).collect::<Vec<_>>()
            )
        });

    // Two columns; three row bands (header + two data rows).
    assert_eq!(table.columns, 2, "grid: {:?}", table.rows);
    assert_eq!(table.rows.len(), 3, "grid: {:?}", table.rows);

    // The header cell spans both columns (no interior vertical rule in row 0).
    let header = &table.rows[0][0];
    assert!(header.text.contains("Header"), "row0: {:?}", table.rows[0]);
    assert_eq!(header.colspan, 2, "spanning header colspan");

    // Data rows split into two single-column cells.
    assert_eq!(table.rows[1][0].colspan, 1);
    assert!(table.rows[1][0].text.contains("Ada"));
    assert!(table.rows[1][1].text.contains("Engineer"));

    // HTML renders the colspan; the geometry/gap path could not have.
    let html = table.to_html();
    assert!(html.contains("colspan=\"2\">Header"), "{html}");
}

#[test]
fn ruled_table_html_renders_the_colspan() {
    let chunks = chunks_of(pdfkit_fixtures::ruled_table_spanning_cell());
    let table = chunks
        .iter()
        .find(|c| c.kind == ElementKind::Table)
        .and_then(|c| c.table.as_ref())
        .expect("a Table chunk");
    // No content is lost across the grid: every data cell text is present.
    let all: String = table
        .rows
        .iter()
        .flatten()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("|");
    for t in ["Header", "Ada", "Engineer", "Linus", "Systems"] {
        assert!(all.contains(t), "missing {t}: {all}");
    }
}

#[test]
fn borderless_table_still_uses_gap_grid() {
    // table_doc has NO vector ruled lines -> the ruled lattice is None and the
    // existing text-gap grid is used unchanged (3x3, all colspan/rowspan 1).
    let chunks = chunks_of(pdfkit_fixtures::table_doc());
    let table = chunks
        .iter()
        .find(|c| c.kind == ElementKind::Table)
        .and_then(|c| c.table.as_ref())
        .expect("a Table chunk");
    assert_eq!(table.columns, 3);
    assert_eq!(table.rows.len(), 3);
    assert!(table
        .rows
        .iter()
        .flatten()
        .all(|c| c.colspan == 1 && c.rowspan == 1));
    assert_eq!(table.rows[0][0].text, "Name");
}

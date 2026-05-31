//! Normalized table grid: cell grid, per-cell colspan/bbox, and HTML/CSV/Markdown
//! serialization. The table_doc fixture is a 3x3 grid (cols at x=72/250/430).

use pdfkit_chunk::{
    chunk_document, to_markdown, Chunk, ChunkOptions, ElementKind, GridCell, Table,
};
use pdfkit_core::{Engine, OpenOptions};

fn chunks_of(bytes: Vec<u8>) -> Vec<Chunk> {
    let doc = Engine::new()
        .unwrap()
        .open(bytes, OpenOptions::default())
        .unwrap();
    chunk_document(&doc, &ChunkOptions::default()).expect("chunk")
}

fn table_chunk(chunks: &[Chunk]) -> &Chunk {
    chunks
        .iter()
        .find(|c| c.kind == ElementKind::Table)
        .expect("a Table chunk")
}

#[test]
fn table_doc_builds_a_3x3_grid() {
    let chunks = chunks_of(pdfkit_fixtures::table_doc());
    let table = table_chunk(&chunks).table.as_ref().expect("grid present");

    assert_eq!(table.columns, 3, "three columns @ x=72/250/430");
    assert_eq!(table.header_rows, 1);
    assert_eq!(table.rows.len(), 3, "three rows");

    let texts: Vec<Vec<&str>> = table
        .rows
        .iter()
        .map(|r| r.iter().map(|c| c.text.as_str()).collect())
        .collect();
    assert_eq!(texts[0], ["Name", "Role", "Level"]);
    assert_eq!(texts[1], ["Ada", "Engineering", "Senior"]);
    assert_eq!(texts[2], ["Linus", "Systems", "Staff"]);

    // Every cell is a single, ordered slot with a real bbox.
    for row in &table.rows {
        for (col, cell) in row.iter().enumerate() {
            assert_eq!(cell.col, col);
            assert_eq!(cell.colspan, 1, "no spans in this fixture");
            assert_eq!(cell.rowspan, 1);
            assert!(cell.bbox[0] < cell.bbox[2] && cell.bbox[1] < cell.bbox[3]);
        }
    }
}

#[test]
fn non_table_chunks_have_no_grid() {
    let chunks = chunks_of(pdfkit_fixtures::multi_heading());
    assert!(chunks.iter().all(|c| c.table.is_none()));
}

#[test]
fn table_html_has_thead_and_escapes() {
    let chunks = chunks_of(pdfkit_fixtures::table_doc());
    let html = table_chunk(&chunks).table.as_ref().unwrap().to_html();
    assert!(html.starts_with("<table><thead>"));
    assert!(html.contains("<th>Name</th><th>Role</th><th>Level</th>"));
    assert!(html.contains("<tbody>"));
    assert!(html.contains("<td>Engineering</td>"));
    // No colspan attribute since every span is 1.
    assert!(!html.contains("colspan"));
}

#[test]
fn table_csv_is_rfc4180() {
    let chunks = chunks_of(pdfkit_fixtures::table_doc());
    let csv = table_chunk(&chunks).table.as_ref().unwrap().to_csv();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "Name,Role,Level");
    assert_eq!(lines[1], "Ada,Engineering,Senior");
    assert_eq!(lines[2], "Linus,Systems,Staff");
}

#[test]
fn two_stacked_tables_stay_separate_chunks() {
    let chunks = chunks_of(pdfkit_fixtures::two_tables());
    let tables: Vec<&Chunk> = chunks
        .iter()
        .filter(|c| c.kind == ElementKind::Table)
        .collect();
    assert_eq!(tables.len(), 2, "two stacked tables must be two chunks");
    for t in tables {
        let grid = t.table.as_ref().expect("each table has its own grid");
        assert_eq!(grid.rows.len(), 2, "each grid matches its own 2-row text");
    }
}

#[test]
fn html_emits_covered_nonempty_cell() {
    // A malformed row where a colspan-2 cell is followed by a non-empty slot it
    // covers: the covered text must not be silently dropped.
    let table = Table {
        columns: 2,
        header_rows: 1,
        rows: vec![vec![
            GridCell {
                text: "A".into(),
                bbox: [0.0, 0.0, 2.0, 1.0],
                col: 0,
                colspan: 2,
                rowspan: 1,
            },
            GridCell {
                text: "X".into(),
                bbox: [1.0, 0.0, 2.0, 1.0],
                col: 1,
                colspan: 1,
                rowspan: 1,
            },
        ]],
    };
    let html = table.to_html();
    assert!(html.contains("colspan=\"2\">A</th>"), "{html}");
    assert!(
        html.contains(">X</th>"),
        "covered non-empty cell dropped: {html}"
    );
}

#[test]
fn table_markdown_matches_grid() {
    let chunks = chunks_of(pdfkit_fixtures::table_doc());
    let md = to_markdown(&chunks);
    assert!(md.contains("| Name | Role | Level |"));
    assert!(md.contains("| --- | --- | --- |"));
    assert!(md.contains("| Ada | Engineering | Senior |"));
    // The caption is still its own chunk, rendered italic — not in the table.
    assert!(md.contains("*Figure 1"));
}

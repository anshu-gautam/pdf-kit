//! `pdfkit-chunk` — structured / RAG chunking (PRD §4.4).
//!
//! Pipeline: pull positioned text runs per page (`Page::text_runs`), group them
//! into lines and blocks, classify each block (heading by relative font size,
//! list by leading glyph), maintain a heading stack for the breadcrumb, and pack
//! blocks into token-sized chunks without crossing block boundaries.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use pdfkit_core::{
    group_runs_into_lines, is_caption, Cell, Document, ImageRegion, Line, PdfError, StructNode,
    TextRun, MAX_SPAN,
};

/// The kind of a structural element a chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ElementKind {
    /// A heading / section title.
    Heading,
    /// Ordinary body text.
    Paragraph,
    /// A list (detected by a leading bullet or number).
    List,
    /// A table (basic detection; not produced in v1).
    Table,
    /// A caption (not produced in v1).
    Caption,
    /// An image / figure region (its text is the caption or alt-text).
    Figure,
}

/// A structured chunk of a document.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Chunk {
    /// Stable content id (a hash of page + kind + text). Identical chunks across
    /// runs get the same id; useful for citation references and dedup.
    pub id: String,
    /// The chunk text (the extracted source content, without any context prefix).
    pub text: String,
    /// Optional situating context for embedding/retrieval (document title +
    /// heading breadcrumb + page), populated when
    /// [`ChunkOptions::contextual_prefix`] is set. Kept separate from `text` so
    /// the provenance offsets stay exact.
    pub context: Option<String>,
    /// One-based page number.
    pub page: usize,
    /// Bounding box `[x0, y0, x1, y1]` in points, if known — the visual
    /// provenance: where on the page this chunk's content sits.
    pub bbox: Option<[f32; 4]>,
    /// The element kind.
    pub kind: ElementKind,
    /// Breadcrumb of enclosing headings (outermost first).
    pub heading_path: Vec<String>,
    /// Char offset of this chunk's `text` within [`document_text`] (the chunks
    /// joined in reading order). With `char_len`, an exact span back into the
    /// reconstructed document text for highlighting / lineage.
    pub char_start: usize,
    /// Char length of `text` (== `text.chars().count()`).
    pub char_len: usize,
    /// Approximate token count.
    pub token_estimate: usize,
    /// The reconstructed cell grid, present only for [`ElementKind::Table`]
    /// chunks. The lossless representation of the table (Markdown/CSV flatten
    /// spans).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub table: Option<Table>,
}

/// A normalized table: a rectangular grid of [`GridCell`]s in reading order.
///
/// Column geometry is inferred from text-gap alignment (no ruled-line parsing
/// yet), so spans are limited: `colspan` is detected from a cell's horizontal
/// overlap with the column slots; `rowspan` is always 1 (true row spans need
/// vector-graphics rules — `TODO(design)`). The first row is treated as the
/// header.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Table {
    /// Number of columns (grid width). Every row has exactly this many cells.
    pub columns: usize,
    /// Number of leading header rows (1 by the first-row heuristic).
    pub header_rows: usize,
    /// Rows of cells, top to bottom; each row has `columns` cells.
    pub rows: Vec<Vec<GridCell>>,
}

/// One cell of a [`Table`] grid.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GridCell {
    /// The cell text (empty for a slot no run landed in).
    pub text: String,
    /// Bounding box `[x0, y0, x1, y1]` in points.
    pub bbox: [f32; 4],
    /// Zero-based column index of the cell's left edge.
    pub col: usize,
    /// Number of columns the cell spans (>= 1).
    pub colspan: usize,
    /// Number of rows the cell spans (always 1 for now).
    pub rowspan: usize,
}

/// Options controlling chunk packing.
#[derive(Debug, Clone)]
pub struct ChunkOptions {
    /// Target size per chunk, in tokens.
    pub target_tokens: usize,
    /// Token overlap carried from the end of one chunk into the next when a
    /// section is split purely by the token budget (0 = none). A value around
    /// 10–15% of `target_tokens` is a common RAG recommendation.
    pub overlap_tokens: usize,
    /// Never split a block across chunks.
    pub respect_boundaries: bool,
    /// When set, populate [`Chunk::context`] with a deterministic situating
    /// prefix (document title + heading breadcrumb + page) for retrieval. The
    /// chunk `text` itself is left unchanged.
    pub contextual_prefix: bool,
}

impl Default for ChunkOptions {
    fn default() -> Self {
        ChunkOptions {
            target_tokens: 512,
            overlap_tokens: 0,
            respect_boundaries: true,
            contextual_prefix: false,
        }
    }
}

/// Chunk a document into structured, token-sized pieces.
pub fn chunk_document(doc: &Document, opts: &ChunkOptions) -> Result<Vec<Chunk>, PdfError> {
    // Prefer the author's logical structure tree when the document is tagged
    // (authoritative heading levels, reading order, table/figure structure);
    // otherwise fall back to the geometry pipeline. Both feed the same packer.
    let blocks = match tagged_blocks(doc) {
        Some(blocks) => blocks,
        None => geometry_blocks(doc)?,
    };
    let mut chunks = pack(blocks, opts);
    finalize_provenance(&mut chunks, doc.metadata().title.as_deref(), opts);
    Ok(chunks)
}

/// Geometry path: positioned runs -> lines -> blocks per page (reading order
/// and multi-column handling live in `pdfkit-core::layout`), plus a Figure block
/// per detected image region, ordered into the page top-to-bottom.
fn geometry_blocks(doc: &Document) -> Result<Vec<Block>, PdfError> {
    // Pass 1: gather runs per page and the global font-size distribution.
    let mut sizes: Vec<f32> = Vec::new();
    let mut pages_runs: Vec<(usize, Vec<TextRun>)> = Vec::new();
    for p in 1..=doc.page_count() {
        let runs = doc.page(p)?.text_runs();
        sizes.extend(runs.iter().map(|r| r.font_size));
        pages_runs.push((p, runs));
    }
    let body = body_size(&sizes);

    // Pass 2: lines -> blocks per page.
    let mut blocks: Vec<Block> = Vec::new();
    for (page, runs) in pages_runs {
        let lines = group_runs_into_lines(runs);
        let mut page_blocks = group_blocks(lines, body, page);
        let figures = doc.page(page)?.image_regions();
        if !figures.is_empty() {
            // A figure adopts its caption as its own text; drop the standalone
            // Caption block for that line so the caption isn't emitted twice.
            let adopted: std::collections::HashSet<&str> = figures
                .iter()
                .filter_map(|f| f.caption.as_deref())
                .collect();
            page_blocks
                .retain(|b| !(b.kind == ElementKind::Caption && adopted.contains(b.text.as_str())));
            page_blocks.extend(figures.into_iter().map(|r| figure_block(r, page)));
            // Order the page top-to-bottom (descending top edge) so each figure
            // sits next to its caption. Only for single-column pages: re-sorting
            // a multi-column page by top edge alone would interleave columns, so
            // there we keep the column reading order and leave figures appended.
            // TODO(design): per-column figure placement.
            if page_blocks.iter().all(|b| b.column == 0) {
                page_blocks
                    .sort_by(|a, b| b.bbox[3].partial_cmp(&a.bbox[3]).unwrap_or(Ordering::Equal));
            }
        }
        blocks.extend(page_blocks);
    }
    Ok(blocks)
}

/// A Figure block from a detected image region; its text is the paired caption.
fn figure_block(region: ImageRegion, page: usize) -> Block {
    let text = match region.caption {
        Some(c) if !c.is_empty() => c,
        _ => "[figure]".to_string(),
    };
    Block {
        text,
        page,
        bbox: region.bbox,
        bbox_known: true,
        size: 1.0,
        kind: ElementKind::Figure,
        last_y: region.bbox[1],
        column: 0,
        lines: 1,
        tabular_lines: 0,
        gap_rows: Vec::new(),
        rows: Vec::new(),
        table: None,
    }
}

/// Tagged path: build blocks from the logical structure tree, or `None` when the
/// document isn't tagged (or the tree has no emittable content — then the caller
/// falls back to geometry).
fn tagged_blocks(doc: &Document) -> Option<Vec<Block>> {
    let root = doc.structure_tree()?;
    let mut blocks = Vec::new();
    let mut last_page = 1usize;
    walk_struct(&root, &mut blocks, &mut last_page);
    (!blocks.is_empty()).then_some(blocks)
}

/// How a (RoleMap-resolved) structure tag becomes a chunk.
enum TagClass {
    /// A heading at this level (1 = top); updates the breadcrumb stack in `pack`.
    Heading(u8),
    /// A block element: emit one chunk aggregating its whole subtree's text.
    Block(ElementKind),
    /// A grouping element: descend without emitting (its content is its children).
    Grouping,
}

/// Classify a standard structure type. Unknown/custom tags group (descend) so
/// their descendants' text is never dropped and no boundary is invented.
fn classify_tag(tag: &str) -> TagClass {
    match tag {
        "H1" => TagClass::Heading(1),
        "H2" => TagClass::Heading(2),
        "H3" => TagClass::Heading(3),
        "H4" => TagClass::Heading(4),
        "H5" => TagClass::Heading(5),
        "H6" => TagClass::Heading(6),
        // An untiered `H` is the document's sole heading tier -> top level.
        "H" => TagClass::Heading(1),
        "P" => TagClass::Block(ElementKind::Paragraph),
        "L" => TagClass::Block(ElementKind::List),
        "Table" => TagClass::Block(ElementKind::Table),
        "Figure" => TagClass::Block(ElementKind::Figure),
        "Caption" => TagClass::Block(ElementKind::Caption),
        "Formula" | "BlockQuote" | "Quote" | "Note" | "Reference" | "BibEntry" => {
            TagClass::Block(ElementKind::Paragraph)
        }
        _ => TagClass::Grouping,
    }
}

/// Walk the structure tree in reading order, emitting one block per heading /
/// block element. `pack` derives the heading breadcrumb from the emitted Heading
/// blocks' sizes, so headings carry a size of `7 - level` (H1 highest).
fn walk_struct(node: &StructNode, blocks: &mut Vec<Block>, last_page: &mut usize) {
    // An unpositioned element takes the last seen page (its neighbors'), rather
    // than colliding on page 1.
    if let Some(p) = node.page {
        *last_page = p;
    }
    let page = node.page.unwrap_or(*last_page);
    match classify_tag(&node.tag) {
        TagClass::Heading(level) => {
            blocks.push(struct_block(
                aggregate(node),
                ElementKind::Heading,
                page,
                7.0 - f32::from(level),
                node.bbox,
            ));
        }
        TagClass::Block(kind) => {
            let mut block = struct_block(block_text(node, kind), kind, page, 1.0, node.bbox);
            if kind == ElementKind::Table {
                block.table = tagged_table_grid(node);
            }
            blocks.push(block);
        }
        TagClass::Grouping => {
            for child in &node.children {
                walk_struct(child, blocks, last_page);
            }
        }
    }
}

/// Text for a block element: a Figure prefers its alt-text; everything else
/// aggregates its subtree.
fn block_text(node: &StructNode, kind: ElementKind) -> String {
    if kind == ElementKind::Figure {
        if let Some(alt) = node.alt.as_deref().filter(|a| !a.is_empty()) {
            return alt.to_string();
        }
        let text = aggregate(node);
        return if text.is_empty() {
            "[figure]".to_string()
        } else {
            text
        };
    }
    aggregate(node)
}

/// A structure element's full text: its own marked-content text plus all
/// descendants', in tree (reading) order, joined by newlines.
fn aggregate(node: &StructNode) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !node.text.is_empty() {
        parts.push(node.text.clone());
    }
    for child in &node.children {
        let child_text = aggregate(child);
        if !child_text.is_empty() {
            parts.push(child_text);
        }
    }
    parts.join("\n")
}

/// Reconstruct a [`Table`] grid from a tagged `Table` element's `TR`/`TH`/`TD`
/// structure, honoring `/ColSpan` and `/RowSpan`. Cells are placed with the
/// standard table layout (a rowspan/colspan cell occupies the slots it covers;
/// later cells flow past them), the grid is made rectangular with empty fillers,
/// and leading all-`TH` rows are the header. Returns `None` if there are no rows.
fn tagged_table_grid(table: &StructNode) -> Option<Table> {
    let mut rows: Vec<&StructNode> = Vec::new();
    collect_rows(table, &mut rows);
    if rows.is_empty() {
        return None;
    }

    // Upper bound on columns: a cell can't create more columns than there are
    // cells in the whole table. Used to clamp a (clamped-but-still-large) span
    // so the materialized grid stays proportional to the real content.
    let max_cols: usize = rows
        .iter()
        .map(|tr| {
            tr.children
                .iter()
                .filter(|c| c.tag == "TH" || c.tag == "TD")
                .count()
        })
        .sum();

    // Place cells into absolute (row, col) slots, tracking slots already covered
    // by a span so later cells skip them.
    let mut occupied: HashSet<(usize, usize)> = HashSet::new();
    let mut cell_at: HashMap<(usize, usize), GridCell> = HashMap::new();
    let mut columns = 0usize;
    let mut header_rows = 0usize;
    let mut in_header_run = true;

    for (r, tr) in rows.iter().enumerate() {
        let cells: Vec<&StructNode> = tr
            .children
            .iter()
            .filter(|c| c.tag == "TH" || c.tag == "TD")
            .collect();
        // A leading run of all-`TH` rows is the header. An empty row carries no
        // cells, so it neither counts as a header row nor ends the run.
        if !cells.is_empty() {
            if in_header_run && cells.iter().all(|c| c.tag == "TH") {
                header_rows += 1;
            } else {
                in_header_run = false;
            }
        }

        let mut c = 0usize;
        for cell in cells {
            while occupied.contains(&(r, c)) {
                c += 1;
            }
            // A span can't usefully exceed the table's own bounds (a cell can't
            // create more columns than there are cells, nor more rows than
            // exist); clamp to those so a large span can't explode the grid or
            // the occupied set. MAX_SPAN (clamped at parse time) is the ceiling.
            let colspan = cell
                .col_span
                .clamp(1, MAX_SPAN)
                .min(max_cols.saturating_sub(c).max(1));
            let rowspan = cell
                .row_span
                .clamp(1, MAX_SPAN)
                .min(rows.len().saturating_sub(r).max(1));
            cell_at.insert(
                (r, c),
                GridCell {
                    text: aggregate(cell),
                    bbox: cell.bbox.unwrap_or([0.0; 4]),
                    col: c,
                    colspan,
                    rowspan,
                },
            );
            for dr in 0..rowspan {
                for dc in 0..colspan {
                    if dr > 0 || dc > 0 {
                        occupied.insert((r + dr, c + dc));
                    }
                }
            }
            c = c.saturating_add(colspan);
            columns = columns.max(c);
        }
    }
    if columns == 0 {
        return None;
    }

    // Materialize a rectangular grid: real cells at their start slot, empty
    // fillers (colspan/rowspan-covered or genuinely empty) elsewhere.
    let grid: Vec<Vec<GridCell>> = (0..rows.len())
        .map(|r| {
            (0..columns)
                .map(|c| {
                    cell_at.get(&(r, c)).cloned().unwrap_or(GridCell {
                        text: String::new(),
                        bbox: [0.0; 4],
                        col: c,
                        colspan: 1,
                        rowspan: 1,
                    })
                })
                .collect()
        })
        .collect();

    Some(Table {
        columns,
        header_rows,
        rows: grid,
    })
}

/// Collect a tagged table's row (`TR`) elements in reading order, descending
/// through grouping wrappers (`THead`/`TBody`/`TFoot`) but not into a row.
fn collect_rows<'a>(node: &'a StructNode, out: &mut Vec<&'a StructNode>) {
    for child in &node.children {
        if child.tag == "TR" {
            out.push(child);
        } else {
            collect_rows(child, out);
        }
    }
}

/// A block from the tagged path. `bbox` is the element's measured box from its
/// marked content (when positioned); `size` is the heading-stack key for `pack`
/// (ignored for non-headings).
fn struct_block(
    text: String,
    kind: ElementKind,
    page: usize,
    size: f32,
    bbox: Option<[f32; 4]>,
) -> Block {
    Block {
        text,
        page,
        bbox: bbox.unwrap_or([0.0; 4]),
        bbox_known: bbox.is_some(),
        size,
        kind,
        last_y: 0.0,
        column: 0,
        lines: 1,
        tabular_lines: 0,
        gap_rows: Vec::new(),
        rows: Vec::new(),
        table: None,
    }
}

/// Fill in the cross-chunk fields once the chunk sequence is final: the exact
/// char span into [`document_text`], a stable id, and (opt-in) the context
/// prefix. Done as a single post-pass so packing stays simple.
fn finalize_provenance(chunks: &mut [Chunk], title: Option<&str>, opts: &ChunkOptions) {
    let mut offset = 0usize;
    for chunk in chunks.iter_mut() {
        chunk.char_len = chunk.text.chars().count();
        chunk.char_start = offset;
        // `document_text` joins chunks with a blank line ("\n\n").
        offset += chunk.char_len + DOCUMENT_TEXT_SEPARATOR.chars().count();
        chunk.id = stable_id(chunk);
        if opts.contextual_prefix {
            chunk.context = Some(context_prefix(title, &chunk.heading_path, chunk.page));
        }
    }
}

/// Separator between chunks in [`document_text`]; `char_start`/`char_len` are
/// computed against a join using exactly this.
const DOCUMENT_TEXT_SEPARATOR: &str = "\n\n";

/// The chunks joined in reading order into one document text. Each chunk's
/// `char_start..char_start + char_len` (counted in chars) slices out exactly
/// that chunk's `text`.
pub fn document_text(chunks: &[Chunk]) -> String {
    let mut out = String::new();
    for (i, chunk) in chunks.iter().enumerate() {
        if i > 0 {
            out.push_str(DOCUMENT_TEXT_SEPARATOR);
        }
        out.push_str(&chunk.text);
    }
    out
}

/// A stable, deterministic content id (FNV-1a 64, hex) over page + kind + text,
/// so the same content yields the same id across runs without a hashing dep.
fn stable_id(chunk: &Chunk) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(PRIME);
        }
    };
    feed(&(chunk.page as u64).to_le_bytes());
    feed(kind_tag(chunk.kind).as_bytes());
    feed(b"\0");
    feed(chunk.text.as_bytes());
    format!("{hash:016x}")
}

/// Stable lowercase tag for a kind (used in ids and Markdown).
fn kind_tag(kind: ElementKind) -> &'static str {
    match kind {
        ElementKind::Heading => "heading",
        ElementKind::Paragraph => "paragraph",
        ElementKind::List => "list",
        ElementKind::Table => "table",
        ElementKind::Caption => "caption",
        ElementKind::Figure => "figure",
    }
}

/// Build the deterministic context prefix: `Title > H1 > H2 (p.N)`.
fn context_prefix(title: Option<&str>, heading_path: &[String], page: usize) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(t) = title {
        if !t.is_empty() {
            parts.push(t);
        }
    }
    parts.extend(heading_path.iter().map(String::as_str));
    let trail = parts.join(" > ");
    if trail.is_empty() {
        format!("(p.{page})")
    } else {
        format!("{trail} (p.{page})")
    }
}

/// Backslash-escape a leading block-level Markdown marker (`#`, `>`, `|`) at the
/// start of each line, so extracted prose that happens to begin with one isn't
/// rendered as a heading, blockquote, or table. Conservative: only the first
/// non-space character of a line is touched.
fn escape_block_markers(text: &str) -> String {
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start();
        out.push_str(&line[..line.len() - trimmed.len()]); // preserve indentation
        if trimmed.starts_with(['#', '>', '|']) {
            out.push('\\');
        }
        out.push_str(trimmed);
    }
    out
}

/// Render chunks to GitHub-flavored Markdown in reading order: headings by
/// depth, captions italic, tab-separated table rows as a pipe table, list items
/// verbatim, and prose with leading Markdown markers escaped. Deterministic and
/// offline; a best-effort human-readable export (the JSON output is lossless).
pub fn to_markdown(chunks: &[Chunk]) -> String {
    let mut out = String::new();
    for chunk in chunks {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        match chunk.kind {
            ElementKind::Heading => {
                let level = (chunk.heading_path.len() + 1).min(6);
                out.push_str(&"#".repeat(level));
                out.push(' ');
                out.push_str(&escape_block_markers(chunk.text.trim()));
            }
            // Captions and figures render as an italic line (a figure's text is
            // its caption / alt-text).
            ElementKind::Caption | ElementKind::Figure => {
                out.push('*');
                out.push_str(chunk.text.trim());
                out.push('*');
            }
            ElementKind::Table => match &chunk.table {
                Some(table) => out.push_str(&grid_to_markdown(table)),
                None => out.push_str(&table_to_markdown(&chunk.text)),
            },
            // List text is rendered verbatim so its leading bullet/number
            // markers survive; prose has its leading markers escaped so extracted
            // content isn't misread as Markdown structure.
            ElementKind::List => out.push_str(chunk.text.trim_end()),
            ElementKind::Paragraph => out.push_str(&escape_block_markers(chunk.text.trim_end())),
        }
    }
    out
}

/// Render a tab-separated table block (rows = lines, cells = tab-split) as a GFM
/// pipe table, escaping cell pipes. The first row is the header. Falls back to
/// the raw text if it isn't actually multi-column.
fn table_to_markdown(text: &str) -> String {
    let rows: Vec<Vec<&str>> = text
        .lines()
        .map(|line| line.split('\t').map(str::trim).collect())
        .collect();
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols < 2 {
        return text.trim_end().to_string();
    }
    let render_row = |cells: &[&str]| -> String {
        let mut s = String::new();
        for c in 0..cols {
            let cell = cells.get(c).copied().unwrap_or("").replace('|', "\\|");
            s.push_str("| ");
            s.push_str(&cell);
            s.push(' ');
        }
        s.push('|');
        s
    };
    let mut out = String::new();
    let mut iter = rows.iter();
    if let Some(header) = iter.next() {
        out.push_str(&render_row(header));
        out.push_str("\n|");
        for _ in 0..cols {
            out.push_str(" --- |");
        }
        for row in iter {
            out.push('\n');
            out.push_str(&render_row(row));
        }
    }
    out
}

/// Render a table grid as a GitHub-flavored Markdown pipe table (first row as
/// header). GFM cannot express spans, so a spanning cell's text lands in its
/// first column and the spanned-over columns render blank — use [`Table::to_html`]
/// or the JSON grid for lossless spans.
fn grid_to_markdown(table: &Table) -> String {
    if table.columns == 0 || table.rows.is_empty() {
        return String::new();
    }
    let render = |row: &[GridCell]| -> String {
        let mut s = String::new();
        for c in 0..table.columns {
            let cell = row.get(c).map(|g| g.text.as_str()).unwrap_or("");
            s.push_str("| ");
            s.push_str(&cell.replace('|', "\\|"));
            s.push(' ');
        }
        s.push('|');
        s
    };
    let mut out = render(&table.rows[0]);
    out.push_str("\n|");
    for _ in 0..table.columns {
        out.push_str(" --- |");
    }
    for row in &table.rows[1..] {
        out.push('\n');
        out.push_str(&render(row));
    }
    out
}

impl Table {
    /// Render as an HTML `<table>` (header rows in `<thead>`), the lossless
    /// textual form — `colspan`/`rowspan` are emitted when > 1. Cell text is
    /// HTML-escaped.
    pub fn to_html(&self) -> String {
        let header_rows = self.header_rows.min(self.rows.len());
        // Remaining rowspan coverage per column, threaded across all rows so a
        // slot covered by a cell above is not re-emitted in the lower row.
        let mut active = vec![0usize; self.columns];
        let mut out = String::from("<table>");
        if header_rows > 0 {
            out.push_str("<thead>");
            for row in &self.rows[..header_rows] {
                out.push_str(&self.row_html(row, "th", &mut active));
            }
            out.push_str("</thead>");
        }
        if self.rows.len() > header_rows {
            out.push_str("<tbody>");
            for row in &self.rows[header_rows..] {
                out.push_str(&self.row_html(row, "td", &mut active));
            }
            out.push_str("</tbody>");
        }
        out.push_str("</table>");
        out
    }

    /// One HTML table row. Emits each cell with its `colspan`/`rowspan` (when
    /// those exceed 1); skips empty filler slots covered by a preceding `colspan`
    /// in this row or by a `rowspan` from a row above (`active` tracks the
    /// latter). A covered slot that nonetheless carries text is still emitted so
    /// no content is dropped.
    fn row_html(&self, row: &[GridCell], tag: &str, active: &mut [usize]) -> String {
        let mut s = String::from("<tr>");
        let mut covered_until = 0usize;
        for col in 0..self.columns {
            // Covered by a rowspan from a previous row: consume one and skip.
            if active.get(col).is_some_and(|&a| a > 0) {
                active[col] -= 1;
                continue;
            }
            let Some(cell) = row.get(col) else { continue };
            // Empty filler covered by a preceding colspan in this row.
            if col < covered_until && cell.text.is_empty() {
                continue;
            }
            let colspan = if col >= covered_until {
                cell.colspan.max(1)
            } else {
                1 // covered-but-non-empty: emit standalone so no text is lost
            };
            let rowspan = cell.rowspan.max(1);
            s.push('<');
            s.push_str(tag);
            if colspan > 1 {
                s.push_str(&format!(" colspan=\"{colspan}\""));
            }
            if rowspan > 1 {
                s.push_str(&format!(" rowspan=\"{rowspan}\""));
            }
            s.push('>');
            s.push_str(&html_escape(&cell.text));
            s.push_str("</");
            s.push_str(tag);
            s.push('>');
            if col >= covered_until {
                covered_until = col + colspan;
            }
            // Mark the columns this cell spans as covered for the next rowspan-1
            // rows below it.
            if rowspan > 1 {
                let end = (col + colspan).min(self.columns);
                for slot in &mut active[col..end] {
                    *slot = rowspan - 1;
                }
            }
        }
        s.push_str("</tr>");
        s
    }

    /// Render as RFC 4180 CSV (one record per row, `columns` fields each). Spans
    /// are flattened: a spanning cell's text is in its starting column and the
    /// spanned-over columns are empty.
    pub fn to_csv(&self) -> String {
        let mut out = String::new();
        for (ri, row) in self.rows.iter().enumerate() {
            if ri > 0 {
                out.push('\n');
            }
            for c in 0..self.columns {
                if c > 0 {
                    out.push(',');
                }
                let text = row.get(c).map(|g| g.text.as_str()).unwrap_or("");
                out.push_str(&csv_field(text));
            }
        }
        out
    }
}

/// HTML-escape the five significant characters.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Quote a CSV field per RFC 4180 when it contains a comma, quote, newline, or
/// leading/trailing whitespace; internal quotes are doubled.
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) || s != s.trim() {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Serialize chunks to pretty JSON. Requires the `serde` feature.
#[cfg(feature = "serde")]
pub fn to_json(chunks: &[Chunk]) -> Result<String, PdfError> {
    serde_json::to_string_pretty(chunks)
        .map_err(|e| PdfError::Backend(format!("serialize chunks to json: {e}")))
}

/// The most common (rounded) font size, treated as body text. Ties break to the
/// smaller size. Falls back to 12.0 when there are no runs.
fn body_size(sizes: &[f32]) -> f32 {
    use std::collections::HashMap;
    let mut counts: HashMap<i32, usize> = HashMap::new();
    for &s in sizes {
        *counts.entry(s.round() as i32).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(size, _)| size as f32)
        .unwrap_or(12.0)
}

/// One merged line's cells, with the line's baseline and size (for cell bboxes).
struct RowCells {
    y: f32,
    size: f32,
    cells: Vec<Cell>,
}

struct Block {
    text: String,
    page: usize,
    bbox: [f32; 4],
    /// Whether `bbox` is a real measured box. Geometry blocks always have one;
    /// tagged-structure blocks don't carry positions, so their chunks get
    /// `bbox: None` (page + char offsets still locate them).
    bbox_known: bool,
    size: f32,
    kind: ElementKind,
    last_y: f32,
    /// Column band of this block (lines from different bands never merge).
    column: usize,
    /// Lines in the block, and how many looked tabular (had a column gap).
    lines: usize,
    tabular_lines: usize,
    /// Per-row x-centers of the wide column gaps, for aligned-table detection.
    gap_rows: Vec<Vec<f32>>,
    /// Per-row cells, kept so a promoted table can be turned into a grid.
    rows: Vec<RowCells>,
    /// The reconstructed grid, set when the block is promoted to a table.
    table: Option<Table>,
}

/// x-positions of column gaps within this many points are treated as the same
/// table column boundary (tolerates rendering jitter / proportional widths).
const ALIGNMENT_TOL: f32 = 8.0;

/// Group lines into blocks and classify each block.
fn group_blocks(lines: Vec<Line>, body: f32, page: usize) -> Vec<Block> {
    let heading_cutoff = body * 1.25;
    let mut blocks: Vec<Block> = Vec::new();

    for line in lines {
        let is_heading = line.size > heading_cutoff;
        let mergeable = !is_heading
            && blocks.last().is_some_and(|b| {
                b.kind != ElementKind::Heading
                    // Never merge across a column boundary (issue #4): lines in
                    // different bands are spatially independent regions.
                    && b.column == line.column
                    && (b.size - line.size).abs() <= line.size * 0.15
                    && (b.last_y - line.y) <= line.size * 1.9 + 4.0
            });

        if mergeable {
            if let Some(b) = blocks.last_mut() {
                let row_is_tabular = !line.gap_xs.is_empty();
                let row = RowCells {
                    y: line.y,
                    size: line.size,
                    cells: line.cells,
                };
                b.text.push('\n');
                b.text.push_str(&line.text);
                b.bbox[0] = b.bbox[0].min(line.x0);
                b.bbox[1] = b.bbox[1].min(line.y);
                b.bbox[2] = b.bbox[2].max(line.x1);
                b.bbox[3] = b.bbox[3].max(line.y + line.size);
                b.last_y = line.y;
                b.lines += 1;
                b.tabular_lines += usize::from(row_is_tabular);
                b.gap_rows.push(line.gap_xs);
                b.rows.push(row);
            }
        } else {
            let kind = if is_heading {
                ElementKind::Heading
            } else {
                classify_text(&line.text)
            };
            blocks.push(Block {
                page,
                bbox: [line.x0, line.y, line.x1, line.y + line.size],
                bbox_known: true,
                size: line.size,
                kind,
                last_y: line.y,
                column: line.column,
                lines: 1,
                tabular_lines: usize::from(!line.gap_xs.is_empty()),
                gap_rows: vec![line.gap_xs],
                rows: vec![RowCells {
                    y: line.y,
                    size: line.size,
                    cells: line.cells,
                }],
                table: None,
                text: line.text,
            });
        }
    }

    // Promote multi-row tabular blocks to Table, and figure/table captions to
    // Caption (headings are left untouched).
    for block in &mut blocks {
        if block.kind == ElementKind::Heading {
            continue;
        }
        // Require both density (most rows tabular) AND alignment (a real column
        // boundary shared by >=2 rows). Alignment is what rejects the
        // false-positive of a justified / hanging-indent paragraph whose single
        // wide gap lands at a different x on each line (issue #5).
        if block.tabular_lines >= 2
            && block.tabular_lines * 2 >= block.lines
            && has_aligned_column(&block.gap_rows)
        {
            block.kind = ElementKind::Table;
            block.table = build_table(&block.rows, block.bbox);
        } else if is_caption(&block.text) {
            block.kind = ElementKind::Caption;
        }
    }
    blocks
}

/// Whether the per-row column-gap x-centers reveal a real table column: a gap
/// position shared (within [`ALIGNMENT_TOL`]) by at least two distinct rows.
fn has_aligned_column(gap_rows: &[Vec<f32>]) -> bool {
    let mut gaps: Vec<(f32, usize)> = gap_rows
        .iter()
        .enumerate()
        .flat_map(|(row, xs)| xs.iter().map(move |&x| (x, row)))
        .collect();
    gaps.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));

    // Walk the sorted gap positions; a cluster spans points within ALIGNMENT_TOL
    // of the cluster's first position. A cluster covering >=2 distinct rows is
    // an aligned column boundary.
    let mut anchor: Option<f32> = None;
    let mut rows: Vec<usize> = Vec::new();
    for (x, row) in gaps {
        match anchor {
            Some(a) if x - a <= ALIGNMENT_TOL => {}
            _ => {
                if distinct_count(&rows) >= 2 {
                    return true;
                }
                anchor = Some(x);
                rows.clear();
            }
        }
        rows.push(row);
    }
    distinct_count(&rows) >= 2
}

/// Number of distinct values in a small unsorted slice.
fn distinct_count(rows: &[usize]) -> usize {
    let mut seen = rows.to_vec();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

/// Build a normalized cell grid for a promoted table block.
///
/// Columns are inferred by clustering cell *left edges* across rows (these are
/// stable even when cell widths vary, unlike gap centers, which is why this is a
/// distinct pass from the gap-center detection in [`has_aligned_column`]). Each
/// cell is placed in the slot its x-center falls in, with `colspan` from how
/// many slots its width covers; missing slots are filled with empty cells so the
/// grid is rectangular. Returns `None` if no aligned columns emerge.
fn build_table(rows: &[RowCells], bbox: [f32; 4]) -> Option<Table> {
    let (left, right) = (bbox[0], bbox[2]);
    // `right > left` is positive on purpose: a NaN/degenerate bbox makes it
    // false, so we bail rather than build a bogus grid.
    let valid = rows.len() >= 2 && right > left;
    if !valid {
        return None;
    }
    let lefts = column_lefts(rows);
    if lefts.is_empty() {
        return None;
    }
    // Slot boundaries: page-left, then each column's left edge, then page-right.
    let mut bounds = vec![left.min(lefts[0])];
    bounds.extend(lefts.iter().skip(1).copied());
    bounds.push(right);
    bounds.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    let slots: Vec<(f32, f32)> = bounds.windows(2).map(|w| (w[0], w[1])).collect();
    let columns = slots.len();
    if columns == 0 {
        return None;
    }

    let grid_rows: Vec<Vec<GridCell>> = rows
        .iter()
        .map(|row| {
            let y0 = row.y;
            let y1 = row.y + row.size.max(1e-3);
            let mut cells: Vec<GridCell> = slots
                .iter()
                .enumerate()
                .map(|(col, &(lo, hi))| GridCell {
                    text: String::new(),
                    bbox: [lo, y0, hi.max(lo + 1e-3), y1],
                    col,
                    colspan: 1,
                    rowspan: 1,
                })
                .collect();
            for cell in &row.cells {
                let (col, span) = place_cell(cell.x0, cell.x1, &slots);
                let gc = &mut cells[col];
                if gc.text.is_empty() {
                    gc.text = cell.text.clone();
                    let cx0 = cell.x0.min(cell.x1);
                    gc.bbox = [cx0, y0, cell.x1.max(cx0 + 1e-3), y1];
                    gc.colspan = span;
                } else {
                    // Two cells in one slot: keep both rather than drop content.
                    gc.text.push(' ');
                    gc.text.push_str(&cell.text);
                    gc.bbox[2] = gc.bbox[2].max(cell.x1);
                    gc.colspan = gc.colspan.max(span);
                }
            }
            cells
        })
        .collect();

    Some(Table {
        columns,
        header_rows: 1,
        rows: grid_rows,
    })
}

/// Cluster cell left edges across rows into column-left positions. A cluster
/// must be shared by >= 2 rows to count (so a lone stray cell doesn't invent a
/// column); the representative is the cluster's smallest (leftmost) x.
fn column_lefts(rows: &[RowCells]) -> Vec<f32> {
    let mut xs: Vec<(f32, usize)> = rows
        .iter()
        .enumerate()
        .flat_map(|(ri, r)| r.cells.iter().map(move |c| (c.x0, ri)))
        .filter(|(x, _)| x.is_finite())
        .collect();
    xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));

    let mut lefts = Vec::new();
    let mut i = 0;
    while i < xs.len() {
        let anchor = xs[i].0;
        let mut j = i;
        let mut cluster_rows = Vec::new();
        while j < xs.len() && xs[j].0 - anchor <= ALIGNMENT_TOL {
            cluster_rows.push(xs[j].1);
            j += 1;
        }
        if distinct_count(&cluster_rows) >= 2 {
            lefts.push(anchor);
        }
        i = j;
    }
    lefts
}

/// Place a cell into the grid: its column is the *leftmost* slot it overlaps by
/// at least half the slot's width, and its colspan is how many slots it so
/// covers. A thin cell that doesn't reach half of any slot falls back to the
/// slot its center sits in, colspan 1. Always returns a valid column and span >= 1
/// so a cell can never vanish or land out of range.
fn place_cell(x0: f32, x1: f32, slots: &[(f32, f32)]) -> (usize, usize) {
    let mut first: Option<usize> = None;
    let mut span = 0usize;
    for (i, &(lo, hi)) in slots.iter().enumerate() {
        let width = hi - lo;
        if width > 1e-3 {
            let overlap = (x1.min(hi) - x0.max(lo)).max(0.0);
            if overlap >= 0.5 * width {
                first.get_or_insert(i);
                span += 1;
            }
        }
    }
    match first {
        Some(i) => (i, span.max(1)),
        None => {
            let center = (x0 + x1) * 0.5;
            let col = slots
                .iter()
                .position(|&(lo, hi)| center >= lo && center < hi)
                .unwrap_or(slots.len().saturating_sub(1));
            (col, 1)
        }
    }
}

/// Classify a non-heading block by its leading glyphs.
fn classify_text(text: &str) -> ElementKind {
    let s = text.trim_start();
    let mut chars = s.chars();
    match chars.next() {
        Some('•' | '-' | '*' | '–' | '·' | '‣' | '◦') => return ElementKind::List,
        Some(c) if c.is_ascii_digit() => {
            // Numbered list: one or more digits followed by '.' or ')'.
            let mut rest = s.chars().skip_while(|c| c.is_ascii_digit());
            if matches!(rest.next(), Some('.') | Some(')')) {
                return ElementKind::List;
            }
        }
        _ => {}
    }
    ElementKind::Paragraph
}

/// Approximate token count (~4 characters per token).
fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4).max(1)
}

/// The trailing ~`tokens` tokens of `text`, snapped to a word boundary. Used to
/// carry overlap context into the next chunk.
fn tail_tokens(text: &str, tokens: usize) -> String {
    let max_chars = tokens.saturating_mul(4);
    if text.chars().count() <= max_chars {
        return text.trim().to_string();
    }
    let start = text
        .char_indices()
        .rev()
        .take(max_chars)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0);
    let tail = &text[start..];
    // Drop a leading partial word so overlap begins on a whole word.
    match tail.find(char::is_whitespace) {
        Some(i) => tail[i..].trim().to_string(),
        None => tail.trim().to_string(),
    }
}

/// Union two bounding boxes.
fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

struct Acc {
    text: String,
    tokens: usize,
    page: usize,
    kind: ElementKind,
    heading_path: Vec<String>,
    bbox: [f32; 4],
    bbox_known: bool,
    table: Option<Table>,
}

impl Acc {
    fn finish(self) -> Chunk {
        Chunk {
            id: String::new(),
            token_estimate: estimate_tokens(&self.text),
            text: self.text,
            context: None,
            page: self.page,
            bbox: self.bbox_known.then_some(self.bbox),
            kind: self.kind,
            heading_path: self.heading_path,
            char_start: 0,
            char_len: 0,
            table: self.table,
        }
    }
}

/// Pack blocks into chunks, maintaining the heading stack for breadcrumbs.
fn pack(blocks: Vec<Block>, opts: &ChunkOptions) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut stack: Vec<(f32, String)> = Vec::new();
    let mut current: Option<Acc> = None;

    for block in blocks {
        if block.kind == ElementKind::Heading {
            if let Some(acc) = current.take() {
                chunks.push(acc.finish());
            }
            // Pop same-or-deeper headings, then this heading's ancestors form
            // its path.
            while stack.last().is_some_and(|(s, _)| *s <= block.size) {
                stack.pop();
            }
            let path: Vec<String> = stack.iter().map(|(_, t)| t.clone()).collect();
            chunks.push(Chunk {
                id: String::new(),
                token_estimate: estimate_tokens(&block.text),
                text: block.text.clone(),
                context: None,
                page: block.page,
                bbox: block.bbox_known.then_some(block.bbox),
                kind: ElementKind::Heading,
                heading_path: path,
                char_start: 0,
                char_len: 0,
                table: None,
            });
            stack.push((block.size, block.text));
            continue;
        }

        let path: Vec<String> = stack.iter().map(|(_, t)| t.clone()).collect();
        let block_tokens = estimate_tokens(&block.text);

        // Decide whether to flush the current chunk, and whether the next chunk
        // is a *continuation* (same page + heading) split purely by the token
        // budget — only then do we carry overlap context across the boundary.
        let (must_flush, continuation, over_budget) = match current.as_ref() {
            Some(c) => {
                let over = c.tokens + block_tokens > opts.target_tokens;
                let same = c.page == block.page && c.heading_path == path;
                // Only merge blocks of the same kind, so tables/captions/lists
                // stay distinct chunks rather than dissolving into paragraphs.
                // Tables never merge with each other either, so a chunk's grid
                // always matches its text (two stacked tables stay separate).
                let flush = !same || c.kind != block.kind || over || c.kind == ElementKind::Table;
                (flush, same, over)
            }
            None => (false, false, false),
        };

        let mut overlap_seed: Option<String> = None;
        if must_flush {
            if let Some(acc) = current.take() {
                // Carry overlap only on a true within-section budget split: same
                // page + heading AND same kind. Otherwise (a kind change) we'd
                // leak e.g. table/caption text into a following paragraph chunk.
                let same_kind = acc.kind == block.kind;
                let finished = acc.finish();
                if opts.overlap_tokens > 0 && continuation && over_budget && same_kind {
                    overlap_seed = Some(tail_tokens(&finished.text, opts.overlap_tokens));
                }
                chunks.push(finished);
            }
        }

        match current.as_mut() {
            Some(c) => {
                c.text.push('\n');
                c.text.push_str(&block.text);
                c.tokens += block_tokens;
                if block.bbox_known {
                    c.bbox = if c.bbox_known {
                        union(c.bbox, block.bbox)
                    } else {
                        block.bbox
                    };
                    c.bbox_known = true;
                }
            }
            None => {
                let text = match overlap_seed {
                    Some(seed) if !seed.is_empty() => format!("{seed}\n{}", block.text),
                    _ => block.text,
                };
                current = Some(Acc {
                    text,
                    // Budget decisions track only *content* tokens: the carried
                    // overlap seed must not reduce the chunk's capacity (else a
                    // seeded chunk starts near target and flushes prematurely into
                    // tiny chunks). token_estimate still counts the seed (finish()
                    // recomputes from the full text).
                    tokens: block_tokens,
                    page: block.page,
                    kind: block.kind,
                    heading_path: path,
                    bbox: block.bbox,
                    bbox_known: block.bbox_known,
                    // The grid belongs to this (table) block; on a later merge we
                    // keep this first grid.
                    table: block.table,
                });
            }
        }
    }

    if let Some(acc) = current.take() {
        chunks.push(acc.finish());
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::{csv_field, has_aligned_column, html_escape, place_cell};
    use pdfkit_core::is_caption;

    #[test]
    fn place_cell_colspan() {
        let slots = [(0.0, 100.0), (100.0, 200.0), (200.0, 300.0)];
        // Fully inside one slot.
        assert_eq!(place_cell(0.0, 95.0, &slots), (0, 1));
        // Covers slot 0 fully and >=half of slot 1 -> colspan 2 from the left.
        assert_eq!(place_cell(0.0, 150.0, &slots), (0, 2));
        // Overlaps slot 0 by < half -> falls back to center slot, colspan 1.
        assert_eq!(place_cell(40.0, 60.0, &slots), (0, 1));
        // A thin cell near a boundary -> center slot, colspan 1.
        assert_eq!(place_cell(145.0, 155.0, &slots), (1, 1));
    }

    #[test]
    fn csv_field_quoting() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_field(" x"), "\" x\"");
        assert_eq!(csv_field("a\nb"), "\"a\nb\"");
    }

    #[test]
    fn html_escape_significant_chars() {
        assert_eq!(html_escape("<a>&\"'"), "&lt;a&gt;&amp;&quot;&#39;");
        assert_eq!(html_escape("plain"), "plain");
    }

    #[test]
    fn aligned_columns_are_a_table() {
        // table_doc-like: gaps at ~two column boundaries, consistent across rows
        // (small jitter within ALIGNMENT_TOL).
        let rows = vec![vec![173.0, 350.0], vec![170.0, 373.0], vec![173.0, 360.0]];
        assert!(has_aligned_column(&rows));
    }

    #[test]
    fn misaligned_gaps_are_not_a_table() {
        // A 2-line justified / hanging-indent paragraph: one wide gap per line,
        // but at unrelated x positions => not a column => not a table (issue #5).
        let rows = vec![vec![250.0], vec![180.0]];
        assert!(!has_aligned_column(&rows));
    }

    #[test]
    fn single_tabular_row_is_not_a_table() {
        // A lone row with internal gaps has no second row to align with.
        let rows = vec![vec![100.0, 300.0]];
        assert!(!has_aligned_column(&rows));
    }

    #[test]
    fn caption_vs_prose() {
        // Real captions: keyword + number + separator.
        assert!(is_caption("Figure 1: A sample diagram."));
        assert!(is_caption("Table 2. Results summary"));
        assert!(is_caption("Fig. 3) Overview of the pipeline"));
        // Prose that merely starts with a caption word is not a caption.
        assert!(!is_caption("Table 1 shows the results below."));
        assert!(!is_caption("The table below lists the fields."));
        assert!(!is_caption("Figure out the answer before proceeding."));
    }

    #[test]
    fn tag_classification() {
        use super::{classify_tag, ElementKind, TagClass};
        assert!(matches!(classify_tag("H1"), TagClass::Heading(1)));
        assert!(matches!(classify_tag("H6"), TagClass::Heading(6)));
        assert!(matches!(classify_tag("H"), TagClass::Heading(1)));
        assert!(matches!(
            classify_tag("P"),
            TagClass::Block(ElementKind::Paragraph)
        ));
        assert!(matches!(
            classify_tag("Table"),
            TagClass::Block(ElementKind::Table)
        ));
        assert!(matches!(
            classify_tag("Figure"),
            TagClass::Block(ElementKind::Figure)
        ));
        assert!(matches!(
            classify_tag("Caption"),
            TagClass::Block(ElementKind::Caption)
        ));
        assert!(matches!(classify_tag("Document"), TagClass::Grouping));
        assert!(matches!(classify_tag("CustomWidget"), TagClass::Grouping));
    }

    #[test]
    fn aggregate_collects_subtree_text_in_order() {
        use super::aggregate;
        use pdfkit_core::StructNode;
        let cell = |t: &str| StructNode {
            tag: "TD".into(),
            raw_tag: "TD".into(),
            text: t.into(),
            alt: None,
            page: Some(1),
            bbox: None,
            col_span: 1,
            row_span: 1,
            children: Vec::new(),
        };
        let row = StructNode {
            tag: "TR".into(),
            raw_tag: "TR".into(),
            text: String::new(),
            alt: None,
            page: Some(1),
            bbox: None,
            col_span: 1,
            row_span: 1,
            children: vec![cell("A"), cell("B")],
        };
        let table = StructNode {
            tag: "Table".into(),
            raw_tag: "Table".into(),
            text: String::new(),
            alt: None,
            page: Some(1),
            bbox: None,
            col_span: 1,
            row_span: 1,
            children: vec![row],
        };
        // Container's own text is empty; descendants collected in tree order.
        assert_eq!(aggregate(&table), "A\nB");

        let empty = StructNode {
            tag: "P".into(),
            raw_tag: "P".into(),
            text: String::new(),
            alt: None,
            page: None,
            bbox: None,
            col_span: 1,
            row_span: 1,
            children: Vec::new(),
        };
        assert_eq!(aggregate(&empty), "");
    }

    #[test]
    fn tagged_table_grid_clamps_pathological_spans() {
        use super::tagged_table_grid;
        use pdfkit_core::StructNode;
        let node = |tag: &str, col_span: usize, children: Vec<StructNode>| StructNode {
            tag: tag.into(),
            raw_tag: tag.into(),
            text: if children.is_empty() {
                "x".into()
            } else {
                String::new()
            },
            alt: None,
            page: Some(1),
            bbox: None,
            col_span,
            row_span: 1,
            children,
        };
        // A single cell claiming a galactic colspan must NOT allocate a galactic
        // grid: columns are bounded by the table's real cell count (here 1).
        let cell = node("TD", usize::MAX, Vec::new());
        let row = node("TR", 1, vec![cell]);
        let table = node("Table", 1, vec![row]);
        let grid = tagged_table_grid(&table).expect("grid");
        assert_eq!(grid.columns, 1, "span clamped to the table's bounds");
        assert_eq!(grid.rows.len(), 1);
        assert_eq!(grid.rows[0][0].text, "x");
    }
}

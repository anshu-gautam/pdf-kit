//! `pdfkit-chunk` — structured / RAG chunking (PRD §4.4).
//!
//! Pipeline: pull positioned text runs per page (`Page::text_runs`), group them
//! into lines and blocks, classify each block (heading by relative font size,
//! list by leading glyph), maintain a heading stack for the breadcrumb, and pack
//! blocks into token-sized chunks without crossing block boundaries.

use std::cmp::Ordering;

use pdfkit_core::{Document, PdfError, TextRun};

/// The kind of a structural element a chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
}

/// A structured chunk of a document.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// The chunk text.
    pub text: String,
    /// One-based page number.
    pub page: usize,
    /// Bounding box `[x0, y0, x1, y1]` in points, if known.
    pub bbox: Option<[f32; 4]>,
    /// The element kind.
    pub kind: ElementKind,
    /// Breadcrumb of enclosing headings (outermost first).
    pub heading_path: Vec<String>,
    /// Approximate token count.
    pub token_estimate: usize,
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
}

impl Default for ChunkOptions {
    fn default() -> Self {
        ChunkOptions {
            target_tokens: 512,
            overlap_tokens: 0,
            respect_boundaries: true,
        }
    }
}

/// Chunk a document into structured, token-sized pieces.
pub fn chunk_document(doc: &Document, opts: &ChunkOptions) -> Result<Vec<Chunk>, PdfError> {
    // Pass 1: gather runs per page and the global font-size distribution.
    let mut sizes: Vec<f32> = Vec::new();
    let mut pages_runs: Vec<(usize, Vec<TextRun>)> = Vec::new();
    for p in 1..=doc.page_count() {
        let runs = doc.page(p)?.text_runs();
        sizes.extend(runs.iter().map(|r| r.font_size));
        pages_runs.push((p, runs));
    }
    let body = body_size(&sizes);

    // Pass 2: lines -> blocks per page, in reading order.
    let mut blocks: Vec<Block> = Vec::new();
    for (page, runs) in pages_runs {
        let lines = group_lines(runs);
        blocks.extend(group_blocks(lines, body, page));
    }

    Ok(pack(blocks, opts))
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

struct Line {
    text: String,
    y: f32,
    x0: f32,
    x1: f32,
    size: f32,
    /// Number of wide horizontal gaps (column separators) on this line.
    columns: usize,
}

struct Block {
    text: String,
    page: usize,
    bbox: [f32; 4],
    size: f32,
    kind: ElementKind,
    last_y: f32,
    /// Lines in the block, and how many looked tabular (had a column gap).
    lines: usize,
    tabular_lines: usize,
}

/// A gap wider than this many times the font size is treated as a table column
/// separator rather than a word space.
const COLUMN_GAP: f32 = 4.0;

/// Group runs into lines by vertical proximity, preserving content-stream order
/// within each line (the reading order), then order lines top-to-bottom.
///
/// Sorting by our approximate x positions would interleave runs whose widths
/// drifted ("emrplo asyees..."); content order avoids that. A space is inserted
/// only at a real horizontal word gap, so per-glyph PDFs don't become
/// "Pr i v i l eg ed".
fn group_lines(runs: Vec<TextRun>) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    for r in runs {
        let y = r.bbox[1];
        let size = r.font_size;
        match lines
            .iter_mut()
            .find(|l| (l.y - y).abs() <= l.size.max(size) * 0.5)
        {
            Some(line) => {
                let gap = r.bbox[0] - line.x1;
                let unit = line.size.max(size);
                if gap > unit * COLUMN_GAP {
                    // Wide gap = column separator: delimit with a tab and count it.
                    line.text.push('\t');
                    line.columns += 1;
                } else if !line.text.is_empty()
                    && !line.text.ends_with([' ', '\t'])
                    && !r.text.starts_with(' ')
                    && gap > unit * 0.25
                {
                    line.text.push(' ');
                }
                line.text.push_str(&r.text);
                line.x0 = line.x0.min(r.bbox[0]);
                // Running max so an out-of-order run can't shrink x1 (which would
                // inflate the next gap / spuriously trip a column) or make x1<x0.
                line.x1 = line.x1.max(r.bbox[2]);
                line.size = line.size.max(size);
            }
            None => lines.push(Line {
                text: r.text,
                y,
                x0: r.bbox[0],
                x1: r.bbox[2],
                size,
                columns: 0,
            }),
        }
    }
    lines.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(Ordering::Equal));
    lines
}

/// Group lines into blocks and classify each block.
fn group_blocks(lines: Vec<Line>, body: f32, page: usize) -> Vec<Block> {
    let heading_cutoff = body * 1.25;
    let mut blocks: Vec<Block> = Vec::new();

    for line in lines {
        let is_heading = line.size > heading_cutoff;
        let mergeable = !is_heading
            && blocks.last().is_some_and(|b| {
                b.kind != ElementKind::Heading
                    && (b.size - line.size).abs() <= line.size * 0.15
                    && (b.last_y - line.y) <= line.size * 1.9 + 4.0
            });

        if mergeable {
            if let Some(b) = blocks.last_mut() {
                b.text.push('\n');
                b.text.push_str(&line.text);
                b.bbox[0] = b.bbox[0].min(line.x0);
                b.bbox[1] = b.bbox[1].min(line.y);
                b.bbox[2] = b.bbox[2].max(line.x1);
                b.bbox[3] = b.bbox[3].max(line.y + line.size);
                b.last_y = line.y;
                b.lines += 1;
                b.tabular_lines += usize::from(line.columns >= 1);
            }
        } else {
            let kind = if is_heading {
                ElementKind::Heading
            } else {
                classify_text(&line.text)
            };
            blocks.push(Block {
                text: line.text,
                page,
                bbox: [line.x0, line.y, line.x1, line.y + line.size],
                size: line.size,
                kind,
                last_y: line.y,
                lines: 1,
                tabular_lines: usize::from(line.columns >= 1),
            });
        }
    }

    // Promote multi-row tabular blocks to Table, and figure/table captions to
    // Caption (headings are left untouched).
    for block in &mut blocks {
        if block.kind == ElementKind::Heading {
            continue;
        }
        if block.tabular_lines >= 2 && block.tabular_lines * 2 >= block.lines {
            block.kind = ElementKind::Table;
        } else if is_caption(&block.text) {
            block.kind = ElementKind::Caption;
        }
    }
    blocks
}

/// A short block of the form "Figure/Fig./Table/Exhibit/Chart/Diagram/Plate <n><sep>"
/// reads as a caption — e.g. "Figure 1:" / "Table 2." The number must be followed
/// by a separator so ordinary prose like "Table 1 shows the results" is NOT
/// treated as a caption.
fn is_caption(text: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "figure", "fig", "table", "exhibit", "chart", "diagram", "plate",
    ];
    let trimmed = text.trim_start();
    if trimmed.split_whitespace().count() > 15 {
        return false;
    }
    let mut words = trimmed.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    if !KEYWORDS.contains(&first.trim_end_matches('.').to_ascii_lowercase().as_str()) {
        return false;
    }
    // The label number must end with (or be followed by) a separator: "1:", "1.",
    // "2)", "3-". Prose ("Table 1 shows ...") has a bare number then a word.
    match words.next() {
        Some(second) if second.chars().next().is_some_and(|c| c.is_ascii_digit()) => {
            second.ends_with([':', '.', ')', '-', '\u{2013}', '\u{2014}'])
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_caption;

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
}

impl Acc {
    fn finish(self) -> Chunk {
        Chunk {
            token_estimate: estimate_tokens(&self.text),
            text: self.text,
            page: self.page,
            bbox: Some(self.bbox),
            kind: self.kind,
            heading_path: self.heading_path,
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
                token_estimate: estimate_tokens(&block.text),
                text: block.text.clone(),
                page: block.page,
                bbox: Some(block.bbox),
                kind: ElementKind::Heading,
                heading_path: path,
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
                let flush = !same || c.kind != block.kind || over;
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
                c.bbox = union(c.bbox, block.bbox);
            }
            None => {
                let (text, tokens) = match overlap_seed {
                    Some(seed) if !seed.is_empty() => {
                        let text = format!("{seed}\n{}", block.text);
                        let tokens = estimate_tokens(&text);
                        (text, tokens)
                    }
                    _ => (block.text, block_tokens),
                };
                current = Some(Acc {
                    text,
                    tokens,
                    page: block.page,
                    kind: block.kind,
                    heading_path: path,
                    bbox: block.bbox,
                });
            }
        }
    }

    if let Some(acc) = current.take() {
        chunks.push(acc.finish());
    }
    chunks
}

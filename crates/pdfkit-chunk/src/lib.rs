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
    /// Token overlap between adjacent chunks (0 = none).
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
}

struct Block {
    text: String,
    page: usize,
    bbox: [f32; 4],
    size: f32,
    kind: ElementKind,
    last_y: f32,
}

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
                let needs_space = !line.text.is_empty()
                    && !line.text.ends_with(' ')
                    && !r.text.starts_with(' ')
                    && gap > line.size.max(size) * 0.25;
                if needs_space {
                    line.text.push(' ');
                }
                line.text.push_str(&r.text);
                line.x0 = line.x0.min(r.bbox[0]);
                line.x1 = r.bbox[2];
                line.size = line.size.max(size);
            }
            None => lines.push(Line {
                text: r.text,
                y,
                x0: r.bbox[0],
                x1: r.bbox[2],
                size,
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
            });
        }
    }
    blocks
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

        let must_flush = current.as_ref().is_some_and(|c| {
            c.page != block.page
                || c.heading_path != path
                || c.tokens + block_tokens > opts.target_tokens
        });
        if must_flush {
            if let Some(acc) = current.take() {
                chunks.push(acc.finish());
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
                current = Some(Acc {
                    text: block.text,
                    tokens: block_tokens,
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

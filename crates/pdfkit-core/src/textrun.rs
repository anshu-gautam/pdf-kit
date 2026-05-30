//! Positioned text-run extraction and layout-aware reflow (PRD §4.4 step 1 +
//! readable text output).
//!
//! Walks a page's content stream tracking the graphics CTM and text state (font,
//! size, text matrix, leading) and emits a [`TextRun`] per show-text operator
//! with an approximate bounding box and effective font size. Text bytes are
//! decoded with the active font's encoding (via lopdf), matching lopdf's
//! `extract_text` decoding while preserving position — so chunking and the
//! reflowed text output get correct glyphs *and* correct layout.

use std::collections::BTreeMap;

use lopdf::content::Content;
use lopdf::{Document as LoDoc, Encoding, Object, ObjectId};

use crate::geometry::Matrix;

/// A run of text with its position and effective font size.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    /// The decoded text of the run.
    pub text: String,
    /// Bounding box `[x0, y0, x1, y1]` in points (PDF user space, origin
    /// bottom-left). `y0` is the baseline; `y1` is `y0 + font_size`.
    pub bbox: [f32; 4],
    /// Effective font size in points (Tf size times the text/CTM vertical scale).
    pub font_size: f32,
}

/// Approximate advance width per character, as a fraction of the font size.
const AVG_GLYPH_WIDTH: f32 = 0.5;

/// Extract positioned text runs from a page's content stream.
pub(crate) fn page_text_runs(doc: &LoDoc, page_id: ObjectId) -> Vec<TextRun> {
    let encodings = font_encodings(doc, page_id);

    let Ok(content) = doc.get_page_content(page_id) else {
        return Vec::new();
    };
    let Ok(parsed) = Content::decode(&content) else {
        return Vec::new();
    };

    let mut runs = Vec::new();
    let mut ctm = Matrix::IDENTITY;
    let mut ctm_stack: Vec<Matrix> = Vec::new();
    let mut tm = Matrix::IDENTITY;
    let mut tlm = Matrix::IDENTITY;
    let mut font_size = 0.0f32;
    let mut leading = 0.0f32;
    let mut encoding: Option<&Encoding> = None;

    for op in &parsed.operations {
        let ops = &op.operands;
        match op.operator.as_str() {
            "q" => ctm_stack.push(ctm),
            "Q" => {
                if let Some(m) = ctm_stack.pop() {
                    ctm = m;
                }
            }
            "cm" => {
                if let Some(m) = Matrix::from_operands(ops) {
                    ctm = m.multiply(&ctm);
                }
            }
            "BT" => {
                tm = Matrix::IDENTITY;
                tlm = Matrix::IDENTITY;
            }
            "Tf" => {
                if let Some(sz) = ops.get(1).and_then(|o| o.as_float().ok()) {
                    font_size = sz;
                }
                encoding = ops
                    .first()
                    .and_then(|o| o.as_name().ok())
                    .and_then(|name| encodings.get(name));
            }
            "TL" => {
                if let Some(l) = ops.first().and_then(|o| o.as_float().ok()) {
                    leading = l;
                }
            }
            "Td" => {
                if let (Some(tx), Some(ty)) = (num(ops, 0), num(ops, 1)) {
                    tlm = Matrix::translation(tx, ty).multiply(&tlm);
                    tm = tlm;
                }
            }
            "TD" => {
                if let (Some(tx), Some(ty)) = (num(ops, 0), num(ops, 1)) {
                    leading = -ty;
                    tlm = Matrix::translation(tx, ty).multiply(&tlm);
                    tm = tlm;
                }
            }
            "Tm" => {
                if let Some(m) = Matrix::from_operands(ops) {
                    tm = m;
                    tlm = m;
                }
            }
            "T*" => {
                tlm = Matrix::translation(0.0, -leading).multiply(&tlm);
                tm = tlm;
            }
            "Tj" => {
                if let Some(bytes) = ops.first().and_then(|o| o.as_str().ok()) {
                    emit(
                        &decode(encoding, bytes),
                        font_size,
                        &ctm,
                        &mut tm,
                        &mut runs,
                    );
                }
            }
            "'" => {
                tlm = Matrix::translation(0.0, -leading).multiply(&tlm);
                tm = tlm;
                if let Some(bytes) = ops.first().and_then(|o| o.as_str().ok()) {
                    emit(
                        &decode(encoding, bytes),
                        font_size,
                        &ctm,
                        &mut tm,
                        &mut runs,
                    );
                }
            }
            "\"" => {
                tlm = Matrix::translation(0.0, -leading).multiply(&tlm);
                tm = tlm;
                if let Some(bytes) = ops.get(2).and_then(|o| o.as_str().ok()) {
                    emit(
                        &decode(encoding, bytes),
                        font_size,
                        &ctm,
                        &mut tm,
                        &mut runs,
                    );
                }
            }
            "TJ" => {
                if let Some(Object::Array(items)) = ops.first() {
                    for item in items {
                        match item {
                            Object::String(bytes, _) => {
                                emit(
                                    &decode(encoding, bytes),
                                    font_size,
                                    &ctm,
                                    &mut tm,
                                    &mut runs,
                                );
                            }
                            other => {
                                if let Ok(adj) = other.as_float() {
                                    let dx = -adj / 1000.0 * font_size;
                                    tm = Matrix::translation(dx, 0.0).multiply(&tm);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    runs
}

/// Build a map of font resource name -> text encoding for the page's fonts.
fn font_encodings(doc: &LoDoc, page_id: ObjectId) -> BTreeMap<Vec<u8>, Encoding<'_>> {
    let mut map = BTreeMap::new();
    if let Ok(fonts) = doc.get_page_fonts(page_id) {
        for (name, font) in fonts {
            if let Ok(encoding) = font.get_font_encoding(doc) {
                map.insert(name, encoding);
            }
        }
    }
    map
}

/// Decode a show-text byte string with the active font encoding, falling back to
/// Latin-1 when no encoding is known or decoding fails.
fn decode(encoding: Option<&Encoding>, bytes: &[u8]) -> String {
    match encoding {
        Some(enc) => LoDoc::decode_text(enc, bytes).unwrap_or_else(|_| latin1(bytes)),
        None => latin1(bytes),
    }
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn num(ops: &[Object], i: usize) -> Option<f32> {
    ops.get(i).and_then(|o| o.as_float().ok())
}

/// Emit a run for already-decoded `text` at the current text matrix, then
/// advance the matrix by the run's approximate width.
fn emit(text: &str, font_size: f32, ctm: &Matrix, tm: &mut Matrix, runs: &mut Vec<TextRun>) {
    let char_count = text.chars().count();
    if char_count == 0 {
        return;
    }

    let combined = tm.multiply(ctm);
    let x0 = combined.e;
    let y0 = combined.f;
    let effective_size = if font_size > 0.0 {
        font_size * combined.vertical_scale()
    } else {
        combined.vertical_scale()
    };
    let width_text = char_count as f32 * font_size * AVG_GLYPH_WIDTH;
    let width_user = width_text * combined.horizontal_scale();

    runs.push(TextRun {
        text: text.to_string(),
        bbox: [x0, y0, x0 + width_user, y0 + effective_size],
        font_size: effective_size,
    });

    *tm = Matrix::translation(width_text, 0.0).multiply(tm);
}

/// Layout-aware, readable text for a page: group runs into lines by vertical
/// position, join words at real horizontal gaps, and separate paragraphs with a
/// blank line. This replaces lopdf's fragment-per-operation output.
pub(crate) fn page_text(doc: &LoDoc, page_id: ObjectId) -> String {
    reflow(page_text_runs(doc, page_id))
}

struct LineAcc {
    y: f32,
    size: f32,
    x1: f32,
    text: String,
}

fn reflow(runs: Vec<TextRun>) -> String {
    use std::cmp::Ordering;

    // Bucket runs into lines by vertical position, *preserving content-stream
    // order within each line* (that is the reading order; sorting by our
    // approximate x positions would interleave runs whose widths drifted).
    let mut lines: Vec<LineAcc> = Vec::new();
    for r in runs {
        let (x0, y, x1) = (r.bbox[0], r.bbox[1], r.bbox[2]);
        let size = r.font_size;
        match lines
            .iter_mut()
            .find(|l| (l.y - y).abs() <= l.size.max(size) * 0.5)
        {
            Some(line) => {
                let gap = x0 - line.x1;
                let need_space = !line.text.is_empty()
                    && !line.text.ends_with(' ')
                    && !r.text.starts_with(' ')
                    && gap > line.size.max(size) * 0.25;
                if need_space {
                    line.text.push(' ');
                }
                line.text.push_str(&r.text);
                line.x1 = x1;
                line.size = line.size.max(size);
            }
            None => lines.push(LineAcc {
                y,
                size,
                x1,
                text: r.text,
            }),
        }
    }

    // Order lines top-to-bottom for output.
    lines.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(Ordering::Equal));

    let mut out = String::new();
    let mut prev_y: Option<f32> = None;
    for line in &lines {
        if let Some(py) = prev_y {
            out.push('\n');
            // A large vertical gap is a paragraph break -> blank line.
            if py - line.y > line.size * 1.8 {
                out.push('\n');
            }
        }
        out.push_str(line.text.trim_end());
        prev_y = Some(line.y);
    }
    out
}

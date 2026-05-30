//! Positioned text-run extraction (PRD §4.4 step 1).
//!
//! Walks a page's content stream tracking the graphics CTM and the text state
//! (font size, text matrix, leading) and emits a [`TextRun`] for each show-text
//! operator, with an approximate bounding box and effective font size. This is
//! what chunking uses to detect headings (by font size) and group blocks.
//!
//! Text bytes are decoded as Latin-1, which is correct for the WinAnsi/Standard
//! simple fonts pdfkit produces and a close-enough fallback otherwise.
// TODO(design): font-metric-aware widths and CID/Type0 font decoding.

use lopdf::content::Content;
use lopdf::{Document as LoDoc, Object, ObjectId};

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
                    show_text(bytes, font_size, &ctm, &mut tm, &mut runs);
                }
            }
            "'" => {
                tlm = Matrix::translation(0.0, -leading).multiply(&tlm);
                tm = tlm;
                if let Some(bytes) = ops.first().and_then(|o| o.as_str().ok()) {
                    show_text(bytes, font_size, &ctm, &mut tm, &mut runs);
                }
            }
            "\"" => {
                tlm = Matrix::translation(0.0, -leading).multiply(&tlm);
                tm = tlm;
                if let Some(bytes) = ops.get(2).and_then(|o| o.as_str().ok()) {
                    show_text(bytes, font_size, &ctm, &mut tm, &mut runs);
                }
            }
            "TJ" => {
                if let Some(Object::Array(items)) = ops.first() {
                    for item in items {
                        match item {
                            Object::String(bytes, _) => {
                                show_text(bytes, font_size, &ctm, &mut tm, &mut runs);
                            }
                            other => {
                                // Kerning adjustment in thousandths of an em:
                                // shifts the next glyph left.
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

fn num(ops: &[Object], i: usize) -> Option<f32> {
    ops.get(i).and_then(|o| o.as_float().ok())
}

/// Emit a run for `bytes` at the current text matrix, then advance the matrix.
fn show_text(bytes: &[u8], font_size: f32, ctm: &Matrix, tm: &mut Matrix, runs: &mut Vec<TextRun>) {
    let text: String = bytes.iter().map(|&b| b as char).collect();
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
        text,
        bbox: [x0, y0, x0 + width_user, y0 + effective_size],
        font_size: effective_size,
    });

    // Advance the text matrix by the run width (in text space).
    *tm = Matrix::translation(width_text, 0.0).multiply(tm);
}

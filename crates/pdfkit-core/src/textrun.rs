//! Positioned text-run extraction and layout-aware reflow (PRD §4.4 step 1 +
//! readable text output).
//!
//! Walks a page's content stream tracking the graphics CTM and text state (font,
//! size, text matrix, leading) and emits a [`TextRun`] per show-text operator
//! with a bounding box and effective font size. Two refinements make the output
//! match the source document:
//! - bytes are decoded with the active font's encoding (via lopdf), matching
//!   lopdf's `extract_text` glyph decoding while preserving position;
//! - glyph advances come from the font's `/Widths` metrics (simple fonts) or
//!   the descendant CIDFont's `/W` + `/DW` metrics (composite Type0 fonts with
//!   an Identity CMap), rather than a fixed estimate, so word gaps — and
//!   therefore inserted spaces — are accurate. Fonts without metrics fall back
//!   to a 0.5-em-per-glyph estimate.

use std::collections::{BTreeMap, HashMap};

use lopdf::content::Content;
use lopdf::{Dictionary, Document as LoDoc, Encoding, Object, ObjectId};

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

/// Fallback advance per glyph, in 1/1000 em, when font metrics are unavailable.
const DEFAULT_ADVANCE: f32 = 500.0;
/// Default glyph width for a CIDFont without `/DW` (PDF spec default is 1000).
const CID_DEFAULT_WIDTH: f32 = 1000.0;

/// How a font turns a show-text byte string into a total advance (1/1000 em).
enum FontAdvance {
    /// Simple (single-byte) font with `/Widths`.
    Simple(SimpleMetrics),
    /// Composite Type0 font with descendant-CIDFont `/W` + `/DW`.
    Cid(CidMetrics),
}

impl FontAdvance {
    /// Total advance, in 1/1000 em, for a show-text byte string.
    fn advance_units(&self, bytes: &[u8]) -> f32 {
        match self {
            FontAdvance::Simple(m) => bytes.iter().map(|&b| m.width(b as i64)).sum(),
            FontAdvance::Cid(c) => c.advance_units(bytes),
        }
    }
}

/// Per-font glyph widths for a simple (single-byte) font, in 1/1000 em.
struct SimpleMetrics {
    first_char: i64,
    widths: Vec<f32>,
    default_width: f32,
}

impl SimpleMetrics {
    /// Advance width (1/1000 em) for a one-byte character code. A width of 0 is
    /// a legitimate value (e.g. combining marks) and is returned as-is; the
    /// default is only used when the code is outside the `/Widths` range.
    fn width(&self, code: i64) -> f32 {
        if code >= self.first_char {
            if let Some(&w) = self.widths.get((code - self.first_char) as usize) {
                return w;
            }
        }
        self.default_width
    }
}

/// Per-CID glyph widths for a composite (Type0) font, in 1/1000 em.
struct CidMetrics {
    /// Default width for any CID not in `widths`.
    default_width: f32,
    /// Explicit per-CID widths from the descendant CIDFont's `/W` array.
    widths: HashMap<u32, f32>,
    /// Whether the Encoding is an Identity CMap, so a 2-byte code *is* the CID.
    /// For non-Identity CMaps we cannot map codes to CIDs without the CMap, so
    /// every 2-byte code gets the default width (still far better than the old
    /// char-count estimate).
    identity: bool,
}

impl CidMetrics {
    /// Total advance for an Identity-encoded (2-byte) show string.
    fn advance_units(&self, bytes: &[u8]) -> f32 {
        let mut sum = 0.0;
        for pair in bytes.chunks_exact(2) {
            let code = (u32::from(pair[0]) << 8) | u32::from(pair[1]);
            sum += if self.identity {
                self.widths
                    .get(&code)
                    .copied()
                    .unwrap_or(self.default_width)
            } else {
                self.default_width
            };
        }
        // A trailing odd byte is malformed; charge it the default width.
        if bytes.len() % 2 == 1 {
            sum += self.default_width;
        }
        sum
    }
}

/// Extract positioned text runs from a page's content stream.
pub(crate) fn page_text_runs(doc: &LoDoc, page_id: ObjectId) -> Vec<TextRun> {
    let Ok(content) = doc.get_page_content(page_id) else {
        return Vec::new();
    };
    let Ok(parsed) = Content::decode(&content) else {
        return Vec::new();
    };
    text_runs_from_content(doc, page_id, &parsed)
}

/// Extract runs from an already-decoded content stream, so callers that also
/// need other content analysis (e.g. image coverage) can share one decode.
pub(crate) fn text_runs_from_content(
    doc: &LoDoc,
    page_id: ObjectId,
    parsed: &Content,
) -> Vec<TextRun> {
    let encodings = font_encodings(doc, page_id);
    let metrics = font_metrics(doc, page_id);

    let mut runs = Vec::new();
    let mut ctm = Matrix::IDENTITY;
    let mut ctm_stack: Vec<Matrix> = Vec::new();
    let mut tm = Matrix::IDENTITY;
    let mut tlm = Matrix::IDENTITY;
    let mut font_size = 0.0f32;
    let mut leading = 0.0f32;
    let mut encoding: Option<&Encoding> = None;
    let mut font: Option<&FontAdvance> = None;

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
                let name = ops.first().and_then(|o| o.as_name().ok());
                encoding = name.and_then(|n| encodings.get(n));
                font = name.and_then(|n| metrics.get(n));
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
                    show_run(bytes, encoding, font, font_size, &ctm, &mut tm, &mut runs);
                }
            }
            "'" => {
                tlm = Matrix::translation(0.0, -leading).multiply(&tlm);
                tm = tlm;
                if let Some(bytes) = ops.first().and_then(|o| o.as_str().ok()) {
                    show_run(bytes, encoding, font, font_size, &ctm, &mut tm, &mut runs);
                }
            }
            "\"" => {
                tlm = Matrix::translation(0.0, -leading).multiply(&tlm);
                tm = tlm;
                if let Some(bytes) = ops.get(2).and_then(|o| o.as_str().ok()) {
                    show_run(bytes, encoding, font, font_size, &ctm, &mut tm, &mut runs);
                }
            }
            "TJ" => {
                if let Some(Object::Array(items)) = ops.first() {
                    for item in items {
                        match item {
                            Object::String(bytes, _) => {
                                show_run(
                                    bytes, encoding, font, font_size, &ctm, &mut tm, &mut runs,
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
pub(crate) fn font_encodings(doc: &LoDoc, page_id: ObjectId) -> BTreeMap<Vec<u8>, Encoding<'_>> {
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

/// Build a map of font resource name -> advance metrics for the page's fonts:
/// `/Widths` for simple fonts, `/W` + `/DW` for composite Type0 fonts. Fonts
/// with no usable metrics (e.g. base-14) are omitted and fall back to the
/// per-glyph estimate.
fn font_metrics(doc: &LoDoc, page_id: ObjectId) -> BTreeMap<Vec<u8>, FontAdvance> {
    let mut map = BTreeMap::new();
    let Ok(fonts) = doc.get_page_fonts(page_id) else {
        return map;
    };
    for (name, font) in fonts {
        let is_type0 = font
            .get(b"Subtype")
            .ok()
            .and_then(|o| o.as_name().ok())
            .map(|s| s == b"Type0")
            .unwrap_or(false);
        if is_type0 {
            if let Some(cid) = build_cid_metrics(doc, font) {
                map.insert(name, FontAdvance::Cid(cid));
            }
            continue;
        }
        // Map every element to a width, resolving indirect references and using
        // 0.0 for any non-numeric entry, so indices stay aligned with char codes
        // (filter_map would silently shift every later width).
        let widths: Vec<f32> = deref_array(doc, font.get(b"Widths").ok())
            .map(|arr| {
                arr.iter()
                    .map(|o| match o {
                        Object::Reference(id) => doc
                            .get_object(*id)
                            .ok()
                            .and_then(|x| x.as_float().ok())
                            .unwrap_or(0.0),
                        other => other.as_float().unwrap_or(0.0),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if widths.is_empty() {
            continue; // e.g. base-14 fonts with built-in metrics
        }
        let first_char = font
            .get(b"FirstChar")
            .ok()
            .and_then(|o| o.as_i64().ok())
            .unwrap_or(0);
        let default_width = font
            .get(b"FontDescriptor")
            .ok()
            .and_then(|o| deref_dict(doc, o))
            .and_then(|d| d.get(b"MissingWidth").ok())
            .and_then(|o| o.as_float().ok())
            .unwrap_or(DEFAULT_ADVANCE);
        map.insert(
            name,
            FontAdvance::Simple(SimpleMetrics {
                first_char,
                widths,
                default_width,
            }),
        );
    }
    map
}

/// Build [`CidMetrics`] for a composite Type0 font from its descendant CIDFont's
/// `/DW` (default width) and `/W` (per-CID widths) arrays. Returns `None` when
/// the descendant font can't be reached.
fn build_cid_metrics(doc: &LoDoc, font: &Dictionary) -> Option<CidMetrics> {
    // Identity-H/V means a 2-byte code is the CID directly; otherwise we lack
    // the CMap and fall back to the default width per code.
    let identity = font
        .get(b"Encoding")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| n.starts_with(b"Identity"))
        .unwrap_or(false);

    let descendants = deref_array(doc, font.get(b"DescendantFonts").ok())?;
    let cid_font = deref_dict(doc, descendants.first()?)?;

    let default_width = cid_font
        .get(b"DW")
        .ok()
        .and_then(|o| o.as_float().ok())
        .unwrap_or(CID_DEFAULT_WIDTH);
    let widths = deref_array(doc, cid_font.get(b"W").ok())
        .map(|w| parse_cid_widths(doc, w))
        .unwrap_or_default();

    Some(CidMetrics {
        default_width,
        widths,
        identity,
    })
}

/// Parse a CIDFont `/W` array into a CID -> width map. The array mixes two
/// forms: `c [w1 w2 ...]` (CIDs `c, c+1, ...`) and `cFirst cLast w` (a constant
/// width for the inclusive range). Widths are in 1/1000 em.
fn parse_cid_widths(doc: &LoDoc, arr: &[Object]) -> HashMap<u32, f32> {
    let mut widths = HashMap::new();
    let mut i = 0;
    while i + 1 < arr.len() {
        // CIDs are 16-bit; a non-CID anchor is malformed, so resync by one
        // element rather than abandoning the rest of the array. Clamping to u16
        // also makes the casts and range loops below overflow-proof.
        let Some(first) = deref_obj(doc, &arr[i])
            .and_then(|o| o.as_i64().ok())
            .and_then(|v| u16::try_from(v).ok())
            .map(u32::from)
        else {
            i += 1;
            continue;
        };
        match deref_obj(doc, &arr[i + 1]) {
            Some(Object::Array(list)) => {
                // `c [w0 w1 ...]`: CIDs c, c+1, ... — stop at the CID ceiling.
                for (k, w) in list.iter().enumerate() {
                    let cid = first + k as u32;
                    if cid > u32::from(u16::MAX) {
                        break;
                    }
                    if let Some(width) = deref_obj(doc, w).and_then(|o| o.as_float().ok()) {
                        widths.insert(cid, width);
                    }
                }
                i += 2;
            }
            _ => {
                // `cFirst cLast w`: a constant width over an inclusive range.
                let last = arr
                    .get(i + 1)
                    .and_then(|o| deref_obj(doc, o)?.as_i64().ok())
                    .and_then(|v| u16::try_from(v).ok())
                    .map(u32::from);
                let width = arr
                    .get(i + 2)
                    .and_then(|o| deref_obj(doc, o)?.as_float().ok());
                match (last, width) {
                    (Some(last), Some(width)) => {
                        for cid in first..=first.max(last) {
                            widths.insert(cid, width);
                        }
                        i += 3;
                    }
                    // Malformed triple: resync on the next element.
                    _ => i += 1,
                }
            }
        }
    }
    widths
}

/// Follow a single indirect reference if `o` is one, else return it directly.
fn deref_obj<'a>(doc: &'a LoDoc, o: &'a Object) -> Option<&'a Object> {
    match o {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    }
}

fn deref_array<'a>(doc: &'a LoDoc, obj: Option<&'a Object>) -> Option<&'a Vec<Object>> {
    match obj? {
        Object::Reference(id) => doc.get_object(*id).ok()?.as_array().ok(),
        other => other.as_array().ok(),
    }
}

fn deref_dict<'a>(doc: &'a LoDoc, obj: &'a Object) -> Option<&'a Dictionary> {
    match obj {
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        other => other.as_dict().ok(),
    }
}

/// Decode a show-text byte string with the active font encoding, falling back to
/// Latin-1 when no encoding is known or decoding fails.
pub(crate) fn decode(encoding: Option<&Encoding>, bytes: &[u8]) -> String {
    let raw = match encoding {
        Some(enc) => LoDoc::decode_text(enc, bytes).unwrap_or_else(|_| latin1(bytes)),
        None => latin1(bytes),
    };
    // A show-text string never carries real line/tab structure — any tab,
    // newline, or carriage return in the decoded glyph text is spurious and
    // would later be misread as a line/row/column break (the structural
    // separators are inserted by layout grouping, not by glyphs). Fold them to
    // a space; leave all other characters untouched.
    if raw.contains(['\n', '\r', '\t']) {
        raw.replace(['\n', '\r', '\t'], " ")
    } else {
        raw
    }
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn num(ops: &[Object], i: usize) -> Option<f32> {
    ops.get(i).and_then(|o| o.as_float().ok())
}

/// Emit a run for a show-text byte string: decode it, advance the text matrix by
/// the glyphs' real (or estimated) widths.
fn show_run(
    bytes: &[u8],
    encoding: Option<&Encoding>,
    font: Option<&FontAdvance>,
    font_size: f32,
    ctm: &Matrix,
    tm: &mut Matrix,
    runs: &mut Vec<TextRun>,
) {
    let text = decode(encoding, bytes);
    if text.is_empty() {
        return;
    }

    // Advance in 1/1000 em: real per-code/per-CID widths when we have metrics,
    // else a flat per-glyph estimate.
    let advance_units: f32 = match font {
        Some(metrics) => metrics.advance_units(bytes),
        None => text.chars().count() as f32 * DEFAULT_ADVANCE,
    };
    let width_text = advance_units / 1000.0 * font_size;

    let combined = tm.multiply(ctm);
    let x0 = combined.e;
    let y0 = combined.f;
    let effective_size = if font_size > 0.0 {
        font_size * combined.vertical_scale()
    } else {
        combined.vertical_scale()
    };
    let width_user = width_text * combined.horizontal_scale();

    runs.push(TextRun {
        text,
        bbox: [x0, y0, x0 + width_user, y0 + effective_size],
        font_size: effective_size,
    });

    *tm = Matrix::translation(width_text, 0.0).multiply(tm);
}

/// Layout-aware, readable text for a page: group runs into reading-order lines
/// (column-aware; see [`crate::layout`]), then join them with a blank line at a
/// paragraph break or a column change. Replaces lopdf's per-operation output.
pub(crate) fn page_text(doc: &LoDoc, page_id: ObjectId) -> String {
    reflow(page_text_runs(doc, page_id))
}

fn reflow(runs: Vec<TextRun>) -> String {
    let lines = crate::layout::group_runs_into_lines(runs);
    let mut out = String::new();
    let mut prev: Option<&crate::layout::Line> = None;
    for line in &lines {
        if let Some(p) = prev {
            out.push('\n');
            // Blank line between paragraphs (a large vertical gap within a
            // column) or whenever the column changes.
            let paragraph_break = if p.column == line.column {
                p.y - line.y > line.size * 1.8
            } else {
                true
            };
            if paragraph_break {
                out.push('\n');
            }
        }
        out.push_str(line.text.trim_end());
        prev = Some(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{parse_cid_widths, CidMetrics};
    use lopdf::{Document as LoDoc, Object};

    #[test]
    fn cid_widths_well_formed_both_w_forms() {
        let doc = LoDoc::new();
        // `c [w0 w1]` then `cFirst cLast w`.
        let arr = vec![
            Object::Integer(0),
            Object::Array(vec![Object::Integer(2000), Object::Integer(1000)]),
            Object::Integer(3),
            Object::Integer(5),
            Object::Integer(700),
        ];
        let w = parse_cid_widths(&doc, &arr);
        assert_eq!(w.get(&0), Some(&2000.0));
        assert_eq!(w.get(&1), Some(&1000.0));
        assert_eq!(w.get(&3), Some(&700.0));
        assert_eq!(w.get(&5), Some(&700.0));
        assert_eq!(w.len(), 5);
    }

    #[test]
    fn cid_widths_reject_negative_and_out_of_range_without_panic() {
        let doc = LoDoc::new();
        // Negative anchor must be skipped, not wrapped to a huge CID (which
        // would overflow-panic in debug on the following `first + k`).
        let neg = vec![
            Object::Integer(-1),
            Object::Array(vec![Object::Integer(500), Object::Integer(600)]),
        ];
        assert!(parse_cid_widths(&doc, &neg).is_empty());
        // An absurd range endpoint must not spin a multi-billion-entry loop.
        let huge = vec![
            Object::Integer(0),
            Object::Integer(2_000_000_000),
            Object::Integer(1000),
        ];
        assert!(parse_cid_widths(&doc, &huge).is_empty());
    }

    #[test]
    fn cid_advance_uses_widths_then_default() {
        let metrics = CidMetrics {
            default_width: 1000.0,
            widths: [(0u32, 2000.0)].into_iter().collect(),
            identity: true,
        };
        // CID 0 (explicit 2000) + CID 1 (default 1000) = 3000.
        assert_eq!(metrics.advance_units(&[0x00, 0x00, 0x00, 0x01]), 3000.0);
        // A trailing odd byte is charged the default width.
        assert_eq!(metrics.advance_units(&[0x00, 0x00, 0x00]), 3000.0);
        // Non-identity: every 2-byte code gets the default width.
        let non_id = CidMetrics {
            default_width: 500.0,
            widths: [(0u32, 2000.0)].into_iter().collect(),
            identity: false,
        };
        assert_eq!(non_id.advance_units(&[0x00, 0x00, 0x00, 0x01]), 1000.0);
    }
}

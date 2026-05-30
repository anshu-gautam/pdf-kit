//! Page classification (PRD §4.1): decide whether a page is text-based,
//! scanned, image-only, or mixed, and expose the raw signals so callers can
//! retune the thresholds.

use std::collections::HashSet;

use lopdf::content::Content;
use lopdf::{Document as LoDoc, Object, ObjectId};

/// The coarse kind of a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageKind {
    /// A real text layer carries the content; little or no image coverage.
    TextBased,
    /// Essentially no text and a single (near) full-page image — a page scan.
    Scanned,
    /// No text but image content that isn't a single full-page scan.
    ImageOnly,
    /// Substantial text *and* substantial image coverage.
    Mixed,
}

/// The raw measurements behind a [`PageKind`]. Exposed so callers can apply
/// their own thresholds instead of (or alongside) [`PageKind`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSignals {
    /// Count of non-whitespace characters recovered from the text layer.
    pub text_char_count: usize,
    /// Number of image XObjects actually drawn on the page.
    pub image_count: usize,
    /// Approximate fraction of the page area covered by images (0.0..=1.0).
    pub image_coverage: f32,
}

// Thresholds. Deliberately conservative; callers can override via the signals.
const TEXT_MIN_CHARS: usize = 50;
const IMAGE_SUBSTANTIAL: f32 = 0.5;
const IMAGE_FULL_PAGE: f32 = 0.6;

/// Map raw signals to a [`PageKind`].
pub(crate) fn classify(signals: &PageSignals) -> PageKind {
    let has_text = signals.text_char_count >= TEXT_MIN_CHARS;
    let substantial_image = signals.image_coverage >= IMAGE_SUBSTANTIAL;

    match (has_text, substantial_image) {
        (true, true) => PageKind::Mixed,
        (true, false) => PageKind::TextBased,
        (false, true) => {
            if signals.image_count == 1 && signals.image_coverage >= IMAGE_FULL_PAGE {
                PageKind::Scanned
            } else {
                PageKind::ImageOnly
            }
        }
        // No text and no substantial image: image content if any, else treat a
        // blank/vector page as text-based (it is not a scan).
        (false, false) => {
            if signals.image_count > 0 {
                PageKind::ImageOnly
            } else {
                PageKind::TextBased
            }
        }
    }
}

/// `(image_count, image_coverage)` for a page: walk the content stream tracking
/// the CTM, and for every `Do` of an image XObject add the area of the unit
/// square under the current transform, divided by the page area.
pub(crate) fn image_signals(
    doc: &LoDoc,
    page_id: ObjectId,
    page_w: f32,
    page_h: f32,
) -> (usize, f32) {
    let image_names = image_xobject_names(doc, page_id);
    if image_names.is_empty() {
        return (0, 0.0);
    }
    let content = match doc.get_page_content(page_id) {
        Ok(c) => c,
        Err(_) => return (0, 0.0),
    };
    let parsed = match Content::decode(&content) {
        Ok(p) => p,
        Err(_) => return (0, 0.0),
    };

    let mut ctm = Matrix::IDENTITY;
    let mut stack: Vec<Matrix> = Vec::new();
    let mut image_area = 0.0f32;
    let mut image_count = 0usize;

    for op in &parsed.operations {
        match op.operator.as_str() {
            "q" => stack.push(ctm),
            "Q" => {
                if let Some(m) = stack.pop() {
                    ctm = m;
                }
            }
            "cm" => {
                if let Some(m) = Matrix::from_operands(&op.operands) {
                    // `cm` premultiplies: CTM' = M_cm x CTM.
                    ctm = m.multiply(&ctm);
                }
            }
            "Do" => {
                if let Some(name) = op.operands.first().and_then(|o| o.as_name().ok()) {
                    if image_names.contains(name) {
                        image_count += 1;
                        image_area += ctm.unit_square_area();
                    }
                }
            }
            _ => {}
        }
    }

    let page_area = (page_w * page_h).max(1.0);
    let coverage = (image_area / page_area).clamp(0.0, 1.0);
    (image_count, coverage)
}

/// Names of image XObjects available to a page, across the page's own Resources
/// and any inherited via the parent chain.
fn image_xobject_names(doc: &LoDoc, page_id: ObjectId) -> HashSet<Vec<u8>> {
    let mut names = HashSet::new();
    let (direct, ref_ids) = match doc.get_page_resources(page_id) {
        Ok(r) => r,
        Err(_) => return names,
    };

    let mut resource_dicts = Vec::new();
    if let Some(d) = direct {
        resource_dicts.push(d);
    }
    for id in ref_ids {
        if let Ok(d) = doc.get_dictionary(id) {
            resource_dicts.push(d);
        }
    }

    for res in resource_dicts {
        let xobjects = res.get(b"XObject").ok().and_then(|o| deref_dict(doc, o));
        let Some(xobjects) = xobjects else { continue };
        for (name, value) in xobjects.iter() {
            if let Ok(id) = value.as_reference() {
                if let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) {
                    if stream
                        .dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .map(|s| s == b"Image")
                        .unwrap_or(false)
                    {
                        names.insert(name.clone());
                    }
                }
            }
        }
    }
    names
}

/// Resolve an object that should be a dictionary, following one reference.
fn deref_dict<'a>(doc: &'a LoDoc, obj: &'a Object) -> Option<&'a lopdf::Dictionary> {
    match obj.as_reference() {
        Ok(id) => doc.get_dictionary(id).ok(),
        Err(_) => obj.as_dict().ok(),
    }
}

/// A 2-D affine transform stored as the six PDF matrix components
/// `[a b c d e f]` (row-vector convention: `[x y 1] * M`).
#[derive(Debug, Clone, Copy)]
struct Matrix {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Matrix {
    const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    fn from_operands(ops: &[Object]) -> Option<Matrix> {
        if ops.len() != 6 {
            return None;
        }
        let n = |i: usize| ops[i].as_float().ok();
        Some(Matrix {
            a: n(0)?,
            b: n(1)?,
            c: n(2)?,
            d: n(3)?,
            e: n(4)?,
            f: n(5)?,
        })
    }

    /// `self * other` (apply `self` first, then `other`).
    fn multiply(&self, other: &Matrix) -> Matrix {
        Matrix {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    /// Area of the transformed unit square = |det| of the linear part.
    fn unit_square_area(&self) -> f32 {
        (self.a * self.d - self.b * self.c).abs()
    }
}

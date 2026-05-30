//! Page classification (PRD §4.1): decide whether a page is text-based,
//! scanned, image-only, or mixed, and expose the raw signals so callers can
//! retune the thresholds.

use std::collections::HashMap;

use lopdf::content::Content;
use lopdf::{Document as LoDoc, Object, ObjectId};

use crate::geometry::Matrix;

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

/// One painting of an image XObject: its stream object id and the CTM in effect
/// (the six PDF matrix components) when it was drawn.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ImageDraw {
    // `id` is only consumed by the native renderer (decoding the image stream).
    #[cfg_attr(not(feature = "render-native"), allow(dead_code))]
    pub id: ObjectId,
    /// `[a b c d e f]` — the transform mapping the image unit square to user
    /// space. The drawn area is `|a*d - b*c|`.
    pub ctm: [f32; 6],
}

/// `(image_count, image_coverage)` for a page: every image `Do` contributes the
/// area of its unit square under the CTM, divided by the page area.
pub(crate) fn image_signals(
    doc: &LoDoc,
    page_id: ObjectId,
    page_w: f32,
    page_h: f32,
) -> (usize, f32) {
    let draws = image_draws(doc, page_id);
    let area: f32 = draws
        .iter()
        .map(|d| (d.ctm[0] * d.ctm[3] - d.ctm[1] * d.ctm[2]).abs())
        .sum();
    let coverage = (area / (page_w * page_h).max(1.0)).clamp(0.0, 1.0);
    (draws.len(), coverage)
}

/// Walk a page's content stream, tracking the CTM (`q`/`Q`/`cm`), and record an
/// [`ImageDraw`] for every `Do` that paints an image XObject.
pub(crate) fn image_draws(doc: &LoDoc, page_id: ObjectId) -> Vec<ImageDraw> {
    let images = image_xobjects(doc, page_id);
    if images.is_empty() {
        return Vec::new();
    }
    let Ok(content) = doc.get_page_content(page_id) else {
        return Vec::new();
    };
    let Ok(parsed) = Content::decode(&content) else {
        return Vec::new();
    };

    let mut ctm = Matrix::IDENTITY;
    let mut stack: Vec<Matrix> = Vec::new();
    let mut draws = Vec::new();

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
                    if let Some(&id) = images.get(name) {
                        draws.push(ImageDraw {
                            id,
                            ctm: ctm.components(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    draws
}

/// Map of image-XObject name -> stream object id, across the page's own
/// Resources and any inherited via the parent chain.
fn image_xobjects(doc: &LoDoc, page_id: ObjectId) -> HashMap<Vec<u8>, ObjectId> {
    let mut map = HashMap::new();
    let Ok((direct, ref_ids)) = doc.get_page_resources(page_id) else {
        return map;
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
        let Some(xobjects) = res.get(b"XObject").ok().and_then(|o| deref_dict(doc, o)) else {
            continue;
        };
        for (name, value) in xobjects.iter() {
            if let Ok(id) = value.as_reference() {
                if let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) {
                    let is_image = stream
                        .dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .map(|s| s == b"Image")
                        .unwrap_or(false);
                    if is_image {
                        map.entry(name.clone()).or_insert(id);
                    }
                }
            }
        }
    }
    map
}

/// Resolve an object that should be a dictionary, following one reference.
fn deref_dict<'a>(doc: &'a LoDoc, obj: &'a Object) -> Option<&'a lopdf::Dictionary> {
    match obj.as_reference() {
        Ok(id) => doc.get_dictionary(id).ok(),
        Err(_) => obj.as_dict().ok(),
    }
}

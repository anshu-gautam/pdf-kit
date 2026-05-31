//! Figure / image-region detection and caption pairing (PRD §4 multimodal).
//!
//! Each image XObject painted on a page is reported as an [`ImageRegion`] with
//! its bounding box (from the CTM in effect at the `Do`) and, when one sits
//! directly above or below it, the text of the nearest caption line. Callers
//! render the page and [`crate::Bitmap::crop_region`] each bbox to a PNG for
//! multimodal pipelines.

use lopdf::{Document as LoDoc, ObjectId};

use crate::layout::{self, Line};

/// A drawn image region on a page and its caption, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageRegion {
    /// Bounding box `[x0, y0, x1, y1]` in points (PDF user space, origin
    /// bottom-left) — the rectangle the image is painted into.
    pub bbox: [f32; 4],
    /// The nearest caption line ("Figure 1: …") directly below or above the
    /// image, if one is present.
    pub caption: Option<String>,
}

/// A caption may sit within this many times its own height of the image edge.
const CAPTION_GAP_EMS: f32 = 3.0;

/// All image regions painted on a page, in content-stream order, each paired
/// with its nearest caption line.
pub(crate) fn image_regions(doc: &LoDoc, page_id: ObjectId) -> Vec<ImageRegion> {
    let draws = crate::classify::image_draws(doc, page_id);
    if draws.is_empty() {
        return Vec::new();
    }
    let lines = layout::group_runs_into_lines(crate::textrun::page_text_runs(doc, page_id));
    draws
        .iter()
        .map(|draw| {
            let bbox = image_bbox(draw.ctm);
            ImageRegion {
                caption: caption_for(bbox, &lines),
                bbox,
            }
        })
        .collect()
}

/// The axis-aligned bounding box of the unit square transformed by `ctm`
/// (covers rotated/skewed placements via the min/max of the four corners).
fn image_bbox(ctm: [f32; 6]) -> [f32; 4] {
    let [a, b, c, d, e, f] = ctm;
    let (mut x0, mut y0) = (f32::INFINITY, f32::INFINITY);
    let (mut x1, mut y1) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let x = a * u + c * v + e;
        let y = b * u + d * v + f;
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    [x0, y0, x1, y1]
}

/// The caption line paired with an image: the nearest caption *below* it
/// (the common convention), falling back to the nearest *above*. The line must
/// horizontally overlap the image and sit within [`CAPTION_GAP_EMS`] of its edge.
fn caption_for(bbox: [f32; 4], lines: &[Line]) -> Option<String> {
    let [fx0, fy0, fx1, fy1] = bbox;
    let mut below: Option<(f32, &Line)> = None;
    let mut above: Option<(f32, &Line)> = None;
    for line in lines {
        if !layout::is_caption(&line.text) {
            continue;
        }
        // Require horizontal overlap with the image.
        if line.x1.min(fx1) <= line.x0.max(fx0) {
            continue;
        }
        let gap = line.size.max(1.0) * CAPTION_GAP_EMS;
        if line.y <= fy0 && line.y >= fy0 - gap {
            let distance = fy0 - line.y;
            if below.is_none_or(|(d, _)| distance < d) {
                below = Some((distance, line));
            }
        } else if line.y >= fy1 && line.y <= fy1 + gap {
            let distance = line.y - fy1;
            if above.is_none_or(|(d, _)| distance < d) {
                above = Some((distance, line));
            }
        }
    }
    below.or(above).map(|(_, line)| line.text.clone())
}

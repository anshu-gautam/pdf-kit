//! Ruled-line (table border) extraction from a page content stream, and the
//! row/column lattice they form (PRD §4.4 table structure).
//!
//! A low-level geometric primitive: it walks the content stream tracking the
//! CTM (like [`crate::classify`]'s image walk), captures axis-aligned line
//! segments from stroked rectangles (`re`) and `m`/`l` subpaths, and clusters
//! them into a [`RuledLattice`]. The chunker uses the lattice to recover true
//! `rowspan`/`colspan` for bordered tables. Stroked borders only — filled
//! rectangles used as rules and rotated tables are out of scope (the segments
//! are skipped, and the consumer falls back to text-gap inference).

use lopdf::content::Content;
use lopdf::{Document as LoDoc, Object, ObjectId};

use crate::geometry::Matrix;

/// An axis-aligned ruled line segment, in PDF user space (origin bottom-left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuledLine {
    /// `true`: vertical (constant `x`, spans `lo..hi` in y); `false`: horizontal.
    pub vertical: bool,
    /// The constant coordinate (`x` if vertical, else `y`).
    pub pos: f32,
    /// Span start (smaller varying endpoint).
    pub lo: f32,
    /// Span end (larger varying endpoint).
    pub hi: f32,
}

/// A page's stroked, axis-aligned ruled lines.
pub fn page_ruled_lines(doc: &LoDoc, page_id: ObjectId) -> Vec<RuledLine> {
    let Ok(content) = doc.get_page_content(page_id) else {
        return Vec::new();
    };
    let Ok(parsed) = Content::decode(&content) else {
        return Vec::new();
    };
    ruled_lines_from_content(&parsed)
}

/// A page with more path operators than this is too complex to scan for a table
/// grid; bail to whatever was found (the lattice gate will likely reject it).
const MAX_PATH_OPS: usize = 20_000;
/// A segment is "axis-aligned" when its off-axis extent is within this (points).
const AXIS_EPS: f32 = 0.5;

/// Extract axis-aligned ruled lines from already-decoded content.
pub(crate) fn ruled_lines_from_content(parsed: &Content) -> Vec<RuledLine> {
    let mut ctm = Matrix::IDENTITY;
    let mut stack: Vec<Matrix> = Vec::new();
    let mut current: Option<(f32, f32)> = None;
    let mut subpath_start: Option<(f32, f32)> = None;
    let mut pending: Vec<RuledLine> = Vec::new();
    let mut out: Vec<RuledLine> = Vec::new();

    for (i, op) in parsed.operations.iter().enumerate() {
        if i >= MAX_PATH_OPS {
            return out;
        }
        let ops = &op.operands;
        match op.operator.as_str() {
            "q" => stack.push(ctm),
            "Q" => {
                if let Some(m) = stack.pop() {
                    ctm = m;
                }
            }
            "cm" => {
                if let Some(m) = Matrix::from_operands(ops) {
                    ctm = m.multiply(&ctm);
                }
            }
            "m" => {
                let p = pt(&ctm, ops, 0, 1);
                current = p;
                subpath_start = p;
            }
            "l" => {
                if let (Some(a), Some(b)) = (current, pt(&ctm, ops, 0, 1)) {
                    push_segment(a, b, &mut pending);
                    current = Some(b);
                }
            }
            "re" => {
                if let Some((x, y, w, h)) = rect(ops) {
                    let c = [
                        ctm.apply(x, y),
                        ctm.apply(x + w, y),
                        ctm.apply(x + w, y + h),
                        ctm.apply(x, y + h),
                    ];
                    push_segment(c[0], c[1], &mut pending);
                    push_segment(c[1], c[2], &mut pending);
                    push_segment(c[2], c[3], &mut pending);
                    push_segment(c[3], c[0], &mut pending);
                }
                current = None;
                subpath_start = None;
            }
            "h" => {
                if let (Some(a), Some(s)) = (current, subpath_start) {
                    push_segment(a, s, &mut pending);
                }
                current = subpath_start;
            }
            // Stroke (incl. close-and-stroke / fill+stroke): the pending path is
            // painted as lines.
            "S" | "s" | "B" | "B*" | "b" | "b*" => {
                if matches!(op.operator.as_str(), "s" | "b" | "b*") {
                    if let (Some(a), Some(start)) = (current, subpath_start) {
                        push_segment(a, start, &mut pending);
                    }
                }
                out.append(&mut pending);
                current = None;
                subpath_start = None;
            }
            // Fill-only or no-paint: the path is a shape, not a rule — discard.
            "f" | "F" | "f*" | "n" => {
                pending.clear();
                current = None;
                subpath_start = None;
            }
            _ => {}
        }
    }
    out
}

/// Transform operands `(ops[ix], ops[iy])` through the CTM, if both are finite.
fn pt(ctm: &Matrix, ops: &[Object], ix: usize, iy: usize) -> Option<(f32, f32)> {
    let x = ops.get(ix)?.as_float().ok()?;
    let y = ops.get(iy)?.as_float().ok()?;
    if !(x.is_finite() && y.is_finite()) {
        return None;
    }
    Some(ctm.apply(x, y))
}

/// The four operands of an `re` (x, y, w, h), if all finite.
fn rect(ops: &[Object]) -> Option<(f32, f32, f32, f32)> {
    let v: Vec<f32> = (0..4).filter_map(|i| ops.get(i)?.as_float().ok()).collect();
    if v.len() == 4 && v.iter().all(|f| f.is_finite()) {
        Some((v[0], v[1], v[2], v[3]))
    } else {
        None
    }
}

/// Push `a -> b` as a [`RuledLine`] if it is axis-aligned (and non-degenerate);
/// diagonal or zero-length segments are skipped.
fn push_segment(a: (f32, f32), b: (f32, f32), out: &mut Vec<RuledLine>) {
    if !(a.0.is_finite() && a.1.is_finite() && b.0.is_finite() && b.1.is_finite()) {
        return;
    }
    let dx = (b.0 - a.0).abs();
    let dy = (b.1 - a.1).abs();
    if dx <= AXIS_EPS && dy > AXIS_EPS {
        out.push(RuledLine {
            vertical: true,
            pos: (a.0 + b.0) * 0.5,
            lo: a.1.min(b.1),
            hi: a.1.max(b.1),
        });
    } else if dy <= AXIS_EPS && dx > AXIS_EPS {
        out.push(RuledLine {
            vertical: false,
            pos: (a.1 + b.1) * 0.5,
            lo: a.0.min(b.0),
            hi: a.0.max(b.0),
        });
    }
}

/// Two ruled positions within this many points are the same grid boundary.
const GRID_TOL: f32 = 3.0;
/// How far from the text block a ruled line may sit and still seed the table's
/// extent (a frame border is typically a few points outside the text).
const SEED_MARGIN: f32 = 24.0;
/// Caps on a lattice (DoS / pathological dense ruling -> reject, fall back).
const MAX_BOUNDARIES: usize = 128;
const MAX_CELLS: usize = 4096;

/// A row/column boundary lattice formed by a page's ruled lines.
#[derive(Debug, Clone, PartialEq)]
pub struct RuledLattice {
    /// Column boundary x's, ascending (>= 2).
    pub col_x: Vec<f32>,
    /// Row boundary y's, descending — row 0 is the top (>= 2).
    pub row_y: Vec<f32>,
    lines: Vec<RuledLine>,
}

/// Build a lattice from the ruled lines overlapping a text block's bbox
/// `[x0, y0, x1, y1]`. Returns `None` unless the lines form a closed grid of
/// `>= 2x2` boundaries over the block (so a stray underline, page border, or a
/// figure box elsewhere can't fabricate a grid).
pub fn lattice_from_lines(lines: &[RuledLine], block: [f32; 4]) -> Option<RuledLattice> {
    let [bx0, by0, bx1, by1] = block;
    if !(bx1 > bx0 && by1 > by0) {
        return None;
    }

    // A table's ruled frame sits OUTSIDE the text (the borders bound the cells,
    // not the glyphs), so a small fixed inflation around the text would miss it.
    // Instead: (1) seed with lines whose perpendicular coordinate is within
    // SEED_MARGIN of the text (catching the nearby frame rules and any text-row
    // line), then (2) let those seed lines' spans reveal the table's true extent
    // — a full-width horizontal rule exposes the far column border even when the
    // text in that column is short — and (3) re-scope to that extent. This
    // self-bounding keeps unrelated lines elsewhere on the page out.
    let in_seed = |l: &RuledLine| {
        let m = SEED_MARGIN;
        if l.vertical {
            l.pos >= bx0 - m && l.pos <= bx1 + m && l.hi >= by0 - m && l.lo <= by1 + m
        } else {
            l.pos >= by0 - m && l.pos <= by1 + m && l.hi >= bx0 - m && l.lo <= bx1 + m
        }
    };
    let (mut ex0, mut ey0, mut ex1, mut ey1) = (bx0, by0, bx1, by1);
    for l in lines.iter().filter(|l| in_seed(l)) {
        let (px0, py0, px1, py1) = if l.vertical {
            (l.pos, l.lo, l.pos, l.hi)
        } else {
            (l.lo, l.pos, l.hi, l.pos)
        };
        ex0 = ex0.min(px0);
        ey0 = ey0.min(py0);
        ex1 = ex1.max(px1);
        ey1 = ey1.max(py1);
    }
    let (xt0, xt1) = (ex0 - GRID_TOL, ex1 + GRID_TOL);
    let (yt0, yt1) = (ey0 - GRID_TOL, ey1 + GRID_TOL);

    // Final scope: lines lying within the table's extent on both axes.
    let scoped: Vec<RuledLine> = lines
        .iter()
        .copied()
        .filter(|l| {
            if l.vertical {
                l.pos >= xt0 && l.pos <= xt1 && l.hi >= yt0 && l.lo <= yt1
            } else {
                l.pos >= yt0 && l.pos <= yt1 && l.hi >= xt0 && l.lo <= xt1
            }
        })
        .collect();

    let col_x = cluster(scoped.iter().filter(|l| l.vertical).map(|l| l.pos));
    let mut row_y = cluster(scoped.iter().filter(|l| !l.vertical).map(|l| l.pos));
    row_y.reverse(); // descending: row 0 at the top

    if col_x.len() < 2 || row_y.len() < 2 {
        return None;
    }
    if col_x.len() > MAX_BOUNDARIES || row_y.len() > MAX_BOUNDARIES {
        return None;
    }
    if (col_x.len() - 1).saturating_mul(row_y.len() - 1) > MAX_CELLS {
        return None;
    }

    let lattice = RuledLattice {
        col_x,
        row_y,
        lines: scoped,
    };
    // Closure: the frame must really be ruled — at least two distinct vertical
    // boundaries span half the row extent and two horizontal boundaries span
    // half the column extent (rejects a lone crossing "+" of two rules).
    if lattice.spanning_count(true) >= 2 && lattice.spanning_count(false) >= 2 {
        Some(lattice)
    } else {
        None
    }
}

/// Cluster coordinates into boundaries, merging values within [`GRID_TOL`].
/// Works in centipoints (i32) so the result is bit-identical across runs.
fn cluster(coords: impl Iterator<Item = f32>) -> Vec<f32> {
    let mut cp: Vec<i32> = coords
        // Bound to far beyond any real page (points) so `v * 100.0` stays well
        // inside i32 — two distinct coords can't saturate to the same key.
        .filter(|v| v.is_finite() && v.abs() < 100_000.0)
        .map(|v| (v * 100.0).round() as i32)
        .collect();
    cp.sort_unstable();
    let tol = (GRID_TOL * 100.0) as i32;
    let mut out: Vec<f32> = Vec::new();
    let mut anchor: Option<i32> = None;
    for v in cp {
        match anchor {
            Some(a) if v - a <= tol => {}
            _ => {
                anchor = Some(v);
                out.push(v as f32 / 100.0);
            }
        }
    }
    out
}

impl RuledLattice {
    /// Number of distinct boundaries (of the given orientation) that have a line
    /// spanning at least half of the opposite axis' extent.
    fn spanning_count(&self, vertical: bool) -> usize {
        let boundaries = if vertical { &self.col_x } else { &self.row_y };
        let (omin, omax) = if vertical {
            (last(&self.row_y), first(&self.row_y)) // y range (row_y is descending)
        } else {
            (first(&self.col_x), last(&self.col_x)) // x range
        };
        let need = (omax - omin) * 0.5;
        boundaries
            .iter()
            .filter(|&&b| {
                self.lines.iter().any(|l| {
                    l.vertical == vertical && (l.pos - b).abs() <= GRID_TOL && (l.hi - l.lo) >= need
                })
            })
            .count()
    }

    /// Whether a vertical boundary at `x` is actually ruled across the band
    /// `[y_bot, y_top]` (covers at least half of it).
    pub fn vertical_present(&self, x: f32, y_top: f32, y_bot: f32) -> bool {
        let need = (y_top - y_bot).abs() * 0.5;
        self.lines.iter().any(|l| {
            l.vertical
                && (l.pos - x).abs() <= GRID_TOL
                && (l.hi.min(y_top) - l.lo.max(y_bot)).max(0.0) >= need
        })
    }

    /// Whether a horizontal boundary at `y` is actually ruled across the span
    /// `[x_lo, x_hi]` (covers at least half of it).
    pub fn horizontal_present(&self, y: f32, x_lo: f32, x_hi: f32) -> bool {
        let need = (x_hi - x_lo).abs() * 0.5;
        self.lines.iter().any(|l| {
            !l.vertical
                && (l.pos - y).abs() <= GRID_TOL
                && (l.hi.min(x_hi) - l.lo.max(x_lo)).max(0.0) >= need
        })
    }
}

fn first(v: &[f32]) -> f32 {
    v.first().copied().unwrap_or(0.0)
}
fn last(v: &[f32]) -> f32 {
    v.last().copied().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_classification() {
        let mut out = Vec::new();
        push_segment((10.0, 10.0), (10.0, 50.0), &mut out); // vertical
        push_segment((10.0, 10.0), (50.0, 10.0), &mut out); // horizontal
        push_segment((10.0, 10.0), (50.0, 50.0), &mut out); // diagonal -> skip
        push_segment((10.0, 10.0), (10.0, 10.0), &mut out); // degenerate -> skip
        assert_eq!(out.len(), 2);
        assert!(out[0].vertical && (out[0].pos - 10.0).abs() < 1e-3);
        assert!(!out[1].vertical && (out[1].pos - 10.0).abs() < 1e-3);
    }

    #[test]
    fn cluster_is_tolerant_and_centipoint_stable() {
        let c = cluster([72.001, 72.004, 73.0, 200.0].into_iter());
        // 72.001 and 72.004 and 73.0 are all within GRID_TOL (3pt) of 72.001.
        assert_eq!(c, vec![72.0, 200.0]);
    }

    /// A closed 2x2 frame (outer rectangle + one interior line each way).
    fn frame() -> Vec<RuledLine> {
        vec![
            RuledLine {
                vertical: true,
                pos: 0.0,
                lo: 0.0,
                hi: 100.0,
            },
            RuledLine {
                vertical: true,
                pos: 50.0,
                lo: 0.0,
                hi: 100.0,
            },
            RuledLine {
                vertical: true,
                pos: 100.0,
                lo: 0.0,
                hi: 100.0,
            },
            RuledLine {
                vertical: false,
                pos: 0.0,
                lo: 0.0,
                hi: 100.0,
            },
            RuledLine {
                vertical: false,
                pos: 50.0,
                lo: 0.0,
                hi: 100.0,
            },
            RuledLine {
                vertical: false,
                pos: 100.0,
                lo: 0.0,
                hi: 100.0,
            },
        ]
    }

    #[test]
    fn lattice_accepts_closed_frame_rejects_sparse() {
        let lat = lattice_from_lines(&frame(), [0.0, 0.0, 100.0, 100.0]).expect("grid");
        assert_eq!(lat.col_x, vec![0.0, 50.0, 100.0]);
        assert_eq!(lat.row_y, vec![100.0, 50.0, 0.0]); // descending

        // A lone cross: two boundaries each way but neither spans -> no grid.
        let cross = vec![
            RuledLine {
                vertical: true,
                pos: 0.0,
                lo: 48.0,
                hi: 52.0,
            },
            RuledLine {
                vertical: true,
                pos: 100.0,
                lo: 48.0,
                hi: 52.0,
            },
            RuledLine {
                vertical: false,
                pos: 50.0,
                lo: 0.0,
                hi: 100.0,
            },
        ];
        assert!(lattice_from_lines(&cross, [0.0, 0.0, 100.0, 100.0]).is_none());
    }

    #[test]
    fn interior_line_presence_queries() {
        let lat = lattice_from_lines(&frame(), [0.0, 0.0, 100.0, 100.0]).unwrap();
        // The interior vertical at x=50 rules the top band -> present.
        assert!(lat.vertical_present(50.0, 100.0, 50.0));
        // No vertical at x=25.
        assert!(!lat.vertical_present(25.0, 100.0, 50.0));
        // Interior horizontal at y=50 rules across the left column.
        assert!(lat.horizontal_present(50.0, 0.0, 50.0));
        assert!(!lat.horizontal_present(75.0, 0.0, 50.0));
    }
}

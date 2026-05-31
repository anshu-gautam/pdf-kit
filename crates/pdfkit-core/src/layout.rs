//! Reading-order line grouping (PRD §4.4 step 1).
//!
//! One shared primitive — [`group_runs_into_lines`] — turns positioned
//! [`TextRun`]s into [`Line`]s in *reading order*. Both `Document::text()`
//! (reflow) and the chunker consume it, so the two never drift apart (the old
//! design kept two copies of this algorithm, one per crate).
//!
//! The grouping has two layers:
//! - **column detection** (PRD §4.4 "multi-column"): a conservative
//!   x-coverage analysis splits a page into vertical bands so that left- and
//!   right-column text sharing a baseline is *not* merged into one scrambled
//!   line. It is deliberately biased toward *not* splitting: any uncertainty
//!   falls back to a single region, which reproduces the previous behavior
//!   exactly. This is what keeps single-column and table pages unchanged.
//! - **baseline bucketing**: within a region, runs are grouped into lines by
//!   vertical proximity, preserving content-stream order within a line (the
//!   reading order — sorting by our approximate x would interleave runs whose
//!   widths drifted), with a space inserted at a real word gap and a tab at a
//!   column-separator gap.

use std::cmp::Ordering;

use crate::textrun::TextRun;

/// One column segment of a line: the text between two column separators, with
/// its horizontal extent. A line with no column gaps has a single cell spanning
/// the whole line. Used by the chunker to reconstruct table grids.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// The cell text (words joined with spaces, no column tabs).
    pub text: String,
    /// Left edge x in points.
    pub x0: f32,
    /// Right edge x in points.
    pub x1: f32,
}

/// A run of text grouped into a single visual line.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The line text, with spaces at word gaps and tabs at column separators.
    pub text: String,
    /// Baseline y in points (PDF user space, origin bottom-left).
    pub y: f32,
    /// Left edge x of the line in points.
    pub x0: f32,
    /// Right edge x of the line in points.
    pub x1: f32,
    /// Effective font size in points (max over the line's runs).
    pub size: f32,
    /// x-centers of the wide ("column separator") gaps on this line, used by
    /// the chunker to detect *aligned* table columns. Its length equals the
    /// number of column separators on the line.
    pub gap_xs: Vec<f32>,
    /// The line split at its column separators (`gap_xs.len() + 1` cells). A
    /// purely additive view — `text`/`gap_xs` are unchanged — so reflow and
    /// table detection are untouched while the chunker can rebuild a cell grid.
    pub cells: Vec<Cell>,
    /// Index of the column band this line belongs to (0 for single-column
    /// pages — then all the column-aware logic in consumers is a no-op).
    pub column: usize,
}

/// A gap wider than this many times the font size is a column separator (a tab
/// in the text and a [`Line::gap_xs`] entry) rather than a word space. Shared
/// so the chunker's table detection and reflow agree on what a column gap is.
pub const COLUMN_GAP: f32 = 4.0;

/// Whether a line reads as a figure/table caption: a short line of the form
/// `Figure/Fig./Table/Exhibit/Chart/Diagram/Plate <n><sep>` — e.g. "Figure 1:"
/// or "Table 2.". The number must be followed by a separator so ordinary prose
/// like "Table 1 shows the results" is not treated as a caption. Shared by the
/// chunker (block classification) and figure/caption pairing.
pub fn is_caption(text: &str) -> bool {
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

/// A gap wider than this many times the font size inserts a single space.
const WORD_GAP: f32 = 0.25;

// --- Column-detection thresholds. Conservative by design: every gate makes a
// split *less* likely, so the worst case is "treated as one column" (today's
// behavior), never a scrambled split. All empirical; validated on the fixtures.
//
// TODO(design): known limitations of this conservative detector, acceptable for
// now (PRD §12): rotated/CTM-scaled text uses unscaled-point thresholds; a tall
// multi-column *table* (>= MIN_LINES_PER_COLUMN rows per cell column) could be
// read column-major; thresholds are validated on synthetic fixtures, not a real
// corpus. Relaxing any of these wants corpus validation first.

/// Max fractional vertical coverage for an x-bin to count as empty (gutter).
/// Lower values risk false gutters in ragged-right single-column text.
const GUTTER_MAX_COVERAGE: f32 = 0.10;
/// Minimum gutter width, in multiples of the median font size.
const GUTTER_MIN_WIDTH_EMS: f32 = 3.0;
/// Each detected band must hold at least this many baseline-lines, else the
/// split is rejected. This is what protects short tables (e.g. a 3-row table)
/// and short single-column pages from being split into columns.
const MIN_LINES_PER_COLUMN: usize = 5;
/// Only 2..=MAX_BANDS columns are considered; more suggests a table or noise.
const MAX_BANDS: usize = 4;

/// Detected column layout: the vertical bands (x-ranges, left to right) and the
/// midpoints of the gutters between them.
struct Columns {
    bands: Vec<(f32, f32)>,
    gutter_mids: Vec<f32>,
}

/// Group positioned runs into lines in reading order.
///
/// On a single-column page (the common case, and any page where column
/// detection is not confident) this is exactly the previous baseline bucketing.
/// On a clean multi-column page it returns the full left column top-to-bottom,
/// then the next column, with any full-width header/footer ordered around them.
pub fn group_runs_into_lines(runs: Vec<TextRun>) -> Vec<Line> {
    if runs.is_empty() {
        return Vec::new();
    }
    let m = median_size(&runs);
    let Some(cols) = detect_columns(&runs, m) else {
        return bucket_region(runs, 0);
    };

    // Classify each run: into the band its x-center falls in, or "full-width"
    // (spanning a gutter). region_y bounds the band-assigned runs.
    let nbands = cols.bands.len();
    let mut band_of: Vec<Option<usize>> = Vec::with_capacity(runs.len());
    let (mut ry_min, mut ry_max) = (f32::INFINITY, f32::NEG_INFINITY);
    for r in &runs {
        if is_spanning(r, &cols.gutter_mids) {
            band_of.push(None);
        } else {
            let cx = (r.bbox[0] + r.bbox[2]) * 0.5;
            band_of.push(Some(band_index(&cols.bands, cx)));
            ry_min = ry_min.min(r.bbox[1]);
            ry_max = ry_max.max(r.bbox[1]);
        }
    }
    // No band-assigned runs (everything spans the gutter): not really columnar.
    if !ry_min.is_finite() {
        return bucket_region(runs, 0);
    }
    // Bail to single-region if a full-width run sits *between* the column rows:
    // the page is not a clean header / columns / footer stack, so column
    // ordering would risk scrambling it. Strictly-interior test.
    for (i, r) in runs.iter().enumerate() {
        if band_of[i].is_none() && r.bbox[1] > ry_min && r.bbox[1] < ry_max {
            return bucket_region(runs, 0);
        }
    }

    // Partition and emit: top matter, then each band left-to-right, then bottom
    // matter — each its own column id so consumers keep them apart.
    let mut top = Vec::new();
    let mut bottom = Vec::new();
    let mut bands: Vec<Vec<TextRun>> = (0..nbands).map(|_| Vec::new()).collect();
    for (i, r) in runs.into_iter().enumerate() {
        match band_of[i] {
            None => {
                if r.bbox[1] >= ry_max {
                    top.push(r);
                } else {
                    bottom.push(r);
                }
            }
            Some(b) => bands[b].push(r),
        }
    }

    let mut out = bucket_region(top, 0);
    for (i, band) in bands.into_iter().enumerate() {
        out.extend(bucket_region(band, i + 1));
    }
    out.extend(bucket_region(bottom, nbands + 1));
    out
}

/// Median font size over non-degenerate runs (size > 0.5pt, so rotated /
/// zero-scaled glyphs don't poison the thresholds). Falls back to 12.0.
fn median_size(runs: &[TextRun]) -> f32 {
    let mut sizes: Vec<f32> = runs
        .iter()
        .map(|r| r.font_size)
        .filter(|s| *s > 0.5)
        .collect();
    if sizes.is_empty() {
        return 12.0;
    }
    sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    sizes[sizes.len() / 2]
}

/// Detect column bands via a 1pt vertical-coverage histogram. Returns `None`
/// (treat as one column) unless it finds 1..=MAX_BANDS-1 interior gutters that
/// each split into bands of >= [`MIN_LINES_PER_COLUMN`] baseline-lines.
fn detect_columns(runs: &[TextRun], m: f32) -> Option<Columns> {
    let (mut x_min, mut x_max) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut y_min, mut y_max) = (f32::INFINITY, f32::NEG_INFINITY);
    for r in runs {
        x_min = x_min.min(r.bbox[0]);
        x_max = x_max.max(r.bbox[2]);
        y_min = y_min.min(r.bbox[1]);
        y_max = y_max.max(r.bbox[3]);
    }
    if !(x_min.is_finite() && x_max.is_finite() && x_max > x_min) {
        return None;
    }
    let height = (y_max - y_min).max(1.0);
    if !height.is_finite() {
        return None;
    }
    // Coordinates should fall within a realistic page span; anything far beyond
    // a page is malformed/untrusted input. Bail to a single region rather than
    // allocate an unbounded coverage histogram (a huge span would be many GB).
    // `f32 as usize` saturates, so this also catches infinities/overflow.
    const MAX_BINS: usize = 20_000; // ~277in at 72pt/in — far wider than any page.
    let nbins = (x_max - x_min).ceil() as usize;
    if nbins == 0 || nbins > MAX_BINS {
        return None;
    }

    // coverage[bin] = summed run heights covering that 1pt column / page height.
    let mut coverage = vec![0.0f32; nbins];
    for r in runs {
        let rh = (r.bbox[3] - r.bbox[1]).max(0.0);
        if !rh.is_finite() {
            continue;
        }
        let lo = (r.bbox[0] - x_min).floor().max(0.0) as usize;
        let hi = ((r.bbox[2] - x_min).ceil() as usize).min(nbins);
        for c in coverage.iter_mut().take(hi).skip(lo) {
            *c += rh;
        }
    }

    // Interior maximal empty runs of width >= GUTTER_MIN_WIDTH_EMS * m.
    let min_width = GUTTER_MIN_WIDTH_EMS * m;
    let mut gutters: Vec<(f32, f32)> = Vec::new();
    let mut i = 0;
    while i < nbins {
        if coverage[i] / height <= GUTTER_MAX_COVERAGE {
            let start = i;
            while i < nbins && coverage[i] / height <= GUTTER_MAX_COVERAGE {
                i += 1;
            }
            // `start > 0 && i < nbins` => interior (not a page margin).
            if start > 0 && i < nbins && (i - start) as f32 >= min_width {
                gutters.push((x_min + start as f32, x_min + i as f32));
            }
        } else {
            i += 1;
        }
    }
    if gutters.is_empty() || gutters.len() >= MAX_BANDS {
        return None;
    }

    let gutter_mids: Vec<f32> = gutters.iter().map(|&(a, b)| (a + b) * 0.5).collect();
    let mut bands = Vec::with_capacity(gutters.len() + 1);
    let mut prev = x_min;
    for &(lo, hi) in &gutters {
        bands.push((prev, lo));
        prev = hi;
    }
    bands.push((prev, x_max));

    for &(lo, hi) in &bands {
        if count_band_lines(runs, lo, hi, &gutter_mids) < MIN_LINES_PER_COLUMN {
            return None;
        }
    }
    Some(Columns { bands, gutter_mids })
}

/// Count distinct baseline-lines among the non-spanning runs whose x-center
/// falls in `[lo, hi)`. Uses the same `|dy| <= 0.5*max(size)` tolerance as
/// [`bucket_region`], so the gate matches the lines that would actually form.
fn count_band_lines(runs: &[TextRun], lo: f32, hi: f32, gutter_mids: &[f32]) -> usize {
    let mut baselines: Vec<(f32, f32)> = Vec::new();
    for r in runs {
        if is_spanning(r, gutter_mids) {
            continue;
        }
        let cx = (r.bbox[0] + r.bbox[2]) * 0.5;
        if cx < lo || cx >= hi {
            continue;
        }
        let (y, size) = (r.bbox[1], r.font_size);
        if baselines
            .iter()
            .any(|&(ly, ls)| (ly - y).abs() <= ls.max(size) * 0.5)
        {
            continue;
        }
        baselines.push((y, size));
    }
    baselines.len()
}

/// A run is "full-width" if its horizontal extent covers a gutter midpoint
/// (i.e. it spans across columns — a header/footer/rule).
fn is_spanning(r: &TextRun, gutter_mids: &[f32]) -> bool {
    gutter_mids
        .iter()
        .any(|&g| r.bbox[0] <= g && g <= r.bbox[2])
}

/// Index of the band whose x-range contains `cx`. A center that lands in a
/// gutter (or just past an edge) joins the band it is physically *nearest*,
/// rather than defaulting to the last band (which would bias right).
fn band_index(bands: &[(f32, f32)], cx: f32) -> usize {
    if let Some(i) = bands.iter().position(|&(lo, hi)| cx >= lo && cx < hi) {
        return i;
    }
    bands
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            dist_to_band(cx, **a)
                .partial_cmp(&dist_to_band(cx, **b))
                .unwrap_or(Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Distance from `cx` to a band's x-range (0 if inside).
fn dist_to_band(cx: f32, (lo, hi): (f32, f32)) -> f32 {
    if cx < lo {
        lo - cx
    } else if cx > hi {
        cx - hi
    } else {
        0.0
    }
}

/// Bucket one region's runs into lines by baseline, preserving content-stream
/// order within each line, then order the lines top-to-bottom. This is the
/// previous bucketing verbatim, plus the `column` tag and `gap_xs` metadata.
fn bucket_region(runs: Vec<TextRun>, column: usize) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    for r in runs {
        let (x0, y, x1) = (r.bbox[0], r.bbox[1], r.bbox[2]);
        let size = r.font_size;
        match lines
            .iter_mut()
            .find(|l| (l.y - y).abs() <= l.size.max(size) * 0.5)
        {
            Some(line) => {
                let gap = x0 - line.x1;
                let unit = line.size.max(size);
                if gap > unit * COLUMN_GAP {
                    // Column separator: tab in the text, record the gap center,
                    // and start a new cell.
                    line.gap_xs.push((line.x1 + x0) * 0.5);
                    line.text.push('\t');
                    line.text.push_str(&r.text);
                    line.cells.push(Cell {
                        text: r.text,
                        x0,
                        x1,
                    });
                } else {
                    let need_space = !line.text.is_empty()
                        && !line.text.ends_with([' ', '\t'])
                        && !r.text.starts_with(' ')
                        && gap > unit * WORD_GAP;
                    if need_space {
                        line.text.push(' ');
                    }
                    line.text.push_str(&r.text);
                    // Append to the open (last) cell, keeping it in lockstep with
                    // `text`; a line always has at least one cell (seeded below).
                    if let Some(cell) = line.cells.last_mut() {
                        if need_space {
                            cell.text.push(' ');
                        }
                        cell.text.push_str(&r.text);
                        cell.x1 = cell.x1.max(x1);
                    }
                }
                line.x0 = line.x0.min(x0);
                // Running max keeps x1 monotonic so an out-of-order run can't
                // shrink it (inflating the next gap) or make x1 < x0.
                line.x1 = line.x1.max(x1);
                line.size = line.size.max(size);
            }
            None => lines.push(Line {
                text: r.text.clone(),
                y,
                x0,
                x1,
                size,
                gap_xs: Vec::new(),
                cells: vec![Cell {
                    text: r.text,
                    x0,
                    x1,
                }],
                column,
            }),
        }
    }
    // y descending, then x0 ascending as a stable tie-break (two runs sharing a
    // baseline keep a deterministic order regardless of float wobble).
    lines.sort_by(|a, b| {
        b.y.partial_cmp(&a.y)
            .unwrap_or(Ordering::Equal)
            .then(a.x0.partial_cmp(&b.x0).unwrap_or(Ordering::Equal))
    });
    lines
}

//! Lay the document model out onto PDF pages via the `pdfkit-edit` create path.
//!
//! A single-column flow with US-Letter pages and 1-inch margins. Paragraphs are
//! greedily word-wrapped (mixing bold/italic spans on a line), headings get
//! larger bold type and spacing, lists get a marker and hanging indent, tables
//! render as a bordered grid, and images are scaled to fit the column. This is
//! intentionally a readable approximation, not a Word layout engine.

use pdfkit_core::PdfError;
use pdfkit_edit::{FontFamily, FontSpec, PageRef, PageSize, PdfBuilder};

use crate::metrics::text_width;
use crate::model::{Align, Block, Cell, Document, Image, Para, ParaKind};

const PAGE_W: f32 = 612.0;
const PAGE_H: f32 = 792.0;
const MARGIN: f32 = 72.0;
const CONTENT_LEFT: f32 = MARGIN;
const CONTENT_RIGHT: f32 = PAGE_W - MARGIN;
const CONTENT_WIDTH: f32 = CONTENT_RIGHT - CONTENT_LEFT;
const TOP: f32 = PAGE_H - MARGIN;
const BOTTOM: f32 = MARGIN;
const LIST_INDENT: f32 = 22.0;

/// Per-paragraph-kind type styling.
struct Style {
    size: f32,
    bold: bool,
    leading: f32,
    space_before: f32,
    space_after: f32,
}

fn style_for(kind: &ParaKind) -> Style {
    match kind {
        ParaKind::Title => Style {
            size: 30.0,
            bold: true,
            leading: 1.18,
            space_before: 4.0,
            space_after: 12.0,
        },
        ParaKind::Heading(1) => Style {
            size: 22.0,
            bold: true,
            leading: 1.2,
            space_before: 14.0,
            space_after: 5.0,
        },
        ParaKind::Heading(2) => Style {
            size: 17.0,
            bold: true,
            leading: 1.2,
            space_before: 12.0,
            space_after: 4.0,
        },
        ParaKind::Heading(3) => Style {
            size: 14.0,
            bold: true,
            leading: 1.25,
            space_before: 10.0,
            space_after: 3.0,
        },
        ParaKind::Heading(_) => Style {
            size: 12.0,
            bold: true,
            leading: 1.3,
            space_before: 8.0,
            space_after: 2.0,
        },
        ParaKind::Body | ParaKind::ListItem { .. } => Style {
            size: 11.0,
            bold: false,
            leading: 1.34,
            space_before: 0.0,
            space_after: 6.0,
        },
    }
}

/// One styled, pre-measured fragment on a line.
struct Piece {
    text: String,
    bold: bool,
    italic: bool,
    width: f32,
    /// Whether a separating space existed before this piece in the source.
    /// Runs that abut with no whitespace (e.g. a bold fragment mid-word) must
    /// not get a spurious space; this records the real boundary.
    space_before: bool,
}

/// Render `doc` to PDF bytes.
pub fn render(doc: &Document) -> Result<Vec<u8>, PdfError> {
    let mut layout = Layout::new();
    for block in &doc.blocks {
        match block {
            Block::Para(p) => layout.draw_paragraph(p),
            Block::Image(img) => layout.draw_image(img),
            Block::Table(t) => layout.draw_table(&t.rows),
        }
    }
    layout.finish()
}

struct Layout {
    builder: PdfBuilder,
    page: PageRef,
    /// Y of the top of the next block (PDF origin is bottom-left).
    y: f32,
    /// Whether anything has been drawn on the current page yet.
    page_dirty: bool,
}

impl Layout {
    fn new() -> Self {
        let mut builder = PdfBuilder::new();
        let page = builder.add_page(PageSize::Letter);
        Layout {
            builder,
            page,
            y: TOP,
            page_dirty: false,
        }
    }

    fn new_page(&mut self) {
        self.page = self.builder.add_page(PageSize::Letter);
        self.y = TOP;
        self.page_dirty = false;
    }

    /// Ensure `height` fits below the cursor; otherwise start a new page. Never
    /// page-breaks an empty page (avoids a leading blank page for tall blocks).
    fn ensure_space(&mut self, height: f32) {
        if self.page_dirty && self.y - height < BOTTOM {
            self.new_page();
        }
    }

    fn font(size: f32, bold: bool, italic: bool) -> FontSpec {
        FontSpec {
            family: FontFamily::Helvetica,
            size,
            bold,
            italic,
        }
    }

    fn draw_paragraph(&mut self, para: &Para) {
        let style = style_for(&para.kind);
        self.y -= style.space_before;

        let (left, marker) = match &para.kind {
            ParaKind::ListItem { marker, level } => (
                CONTENT_LEFT + f32::from(*level) * LIST_INDENT,
                Some(marker.clone()),
            ),
            _ => (CONTENT_LEFT, None),
        };

        // The marker sits in the gutter; wrapped text hangs to its right.
        let hang = match &marker {
            Some(m) => text_width(m, style.size, false),
            None => 0.0,
        };
        let text_left = left + hang;
        let max_width = (CONTENT_RIGHT - text_left).max(1.0);

        let lines = wrap(&para.runs, style.size, style.bold, max_width);
        let lines = if lines.is_empty() {
            vec![Vec::new()]
        } else {
            lines
        };

        let line_h = style.size * style.leading;
        for (i, line) in lines.iter().enumerate() {
            self.ensure_space(line_h);
            let baseline = self.y - style.size * 0.8;
            if i == 0 {
                if let Some(m) = &marker {
                    // Draw the list marker once, in the gutter.
                    self.builder.draw_text(
                        self.page,
                        m,
                        (left, baseline),
                        Self::font(style.size, false, false),
                    );
                }
            }
            self.draw_line(line, text_left, max_width, para.align, baseline, style.size);
            self.y -= line_h;
            self.page_dirty = true;
        }
        self.y -= style.space_after;
    }

    /// Place the pieces of one wrapped line, honoring alignment.
    fn draw_line(
        &mut self,
        pieces: &[Piece],
        x_left: f32,
        max_width: f32,
        align: Align,
        baseline: f32,
        size: f32,
    ) {
        if pieces.is_empty() {
            return;
        }
        let space_w = text_width(" ", size, false);
        // Only count a separating space where the source actually had one, so
        // abutting runs (no whitespace between them) don't gain phantom width.
        let gaps = pieces.iter().skip(1).filter(|p| p.space_before).count();
        let total: f32 = pieces.iter().map(|p| p.width).sum::<f32>() + space_w * gaps as f32;
        let mut x = match align {
            Align::Center => x_left + (max_width - total) / 2.0,
            Align::Right => x_left + (max_width - total),
            Align::Left | Align::Justify => x_left,
        };
        for (i, p) in pieces.iter().enumerate() {
            if i > 0 && p.space_before {
                x += space_w;
            }
            self.builder.draw_text(
                self.page,
                &p.text,
                (x, baseline),
                Self::font(size, p.bold, p.italic),
            );
            x += p.width;
        }
    }

    fn draw_image(&mut self, img: &Image) {
        let (mut w, mut h) = match (img.pt_w, img.pt_h) {
            (Some(w), Some(h)) if w > 0.0 && h > 0.0 => (w, h),
            // No extent: assume the image's pixels are 96 dpi.
            _ => (img.px_w as f32 * 0.75, img.px_h as f32 * 0.75),
        };
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        if w > CONTENT_WIDTH {
            let s = CONTENT_WIDTH / w;
            w *= s;
            h *= s;
        }
        let max_h = TOP - BOTTOM;
        if h > max_h {
            let s = max_h / h;
            w *= s;
            h *= s;
        }
        self.ensure_space(h + 6.0);
        let y_top = self.y;
        let rect = [CONTENT_LEFT, y_top - h, CONTENT_LEFT + w, y_top];
        // A single undecodable image shouldn't abort the whole document.
        let _ = self.builder.place_image(self.page, &img.png, rect);
        self.y -= h + 8.0;
        self.page_dirty = true;
    }

    fn draw_table(&mut self, rows: &[Vec<Cell>]) {
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        if cols == 0 {
            return;
        }
        let col_w = CONTENT_WIDTH / cols as f32;
        let pad = 4.0;
        let size = 10.5;
        let line_h = size * 1.3;

        // How many text lines a band can hold on one fresh page's content area
        // (at least 1, so a row always makes progress even when very tall).
        let lines_per_page = (((TOP - BOTTOM - 2.0 * pad) / line_h).floor() as usize).max(1);

        for row in rows {
            // Wrap each cell; the row is as tall as its tallest cell.
            let cell_lines: Vec<Vec<Vec<Piece>>> = (0..cols)
                .map(|ci| match row.get(ci) {
                    Some(cell) => wrap_cell(cell, size, col_w - 2.0 * pad),
                    None => Vec::new(),
                })
                .collect();
            let max_lines = cell_lines.iter().map(Vec::len).max().unwrap_or(0).max(1);

            // Draw the row band by band so a row taller than the page splits
            // across pages instead of overflowing the bottom margin (mirroring
            // the image height clamp). A row that fits is a single band.
            let mut start = 0usize;
            while start < max_lines {
                let avail = ((self.y - BOTTOM - 2.0 * pad) / line_h).floor();
                let fit_here = if avail >= 1.0 {
                    (avail as usize).min(max_lines - start)
                } else {
                    0
                };
                let band = if fit_here == 0 {
                    if self.page_dirty {
                        self.new_page();
                    }
                    (max_lines - start).min(lines_per_page).max(1)
                } else {
                    fit_here
                };

                let band_h = band as f32 * line_h + 2.0 * pad;
                let y_top = self.y;
                let y_bot = y_top - band_h;

                for (ci, lines) in cell_lines.iter().enumerate() {
                    let cx = CONTENT_LEFT + ci as f32 * col_w;
                    // Cell borders (overdrawing shared edges is fine).
                    self.cell_border(cx, y_bot, cx + col_w, y_top);
                    let mut baseline = y_top - pad - size * 0.8;
                    for line in lines.iter().skip(start).take(band) {
                        self.draw_line(
                            line,
                            cx + pad,
                            col_w - 2.0 * pad,
                            Align::Left,
                            baseline,
                            size,
                        );
                        baseline -= line_h;
                    }
                }
                self.y = y_bot;
                self.page_dirty = true;
                start += band;
            }
        }
        self.y -= 8.0;
    }

    fn cell_border(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let (w, g) = (0.5, 0.55);
        self.builder.draw_line(self.page, (x0, y0), (x1, y0), w, g);
        self.builder.draw_line(self.page, (x0, y1), (x1, y1), w, g);
        self.builder.draw_line(self.page, (x0, y0), (x0, y1), w, g);
        self.builder.draw_line(self.page, (x1, y0), (x1, y1), w, g);
    }

    fn finish(self) -> Result<Vec<u8>, PdfError> {
        let mut out = Vec::new();
        self.builder.save(&mut out)?;
        Ok(out)
    }
}

/// Greedily wrap styled runs into lines that fit `max_width`. `base_bold` forces
/// bold (for headings) on top of each run's own styling.
fn wrap(
    runs: &[crate::model::TextRun],
    size: f32,
    base_bold: bool,
    max_width: f32,
) -> Vec<Vec<Piece>> {
    let space_w = text_width(" ", size, false);
    let mut lines: Vec<Vec<Piece>> = Vec::new();
    let mut cur: Vec<Piece> = Vec::new();
    let mut cur_w = 0.0f32;
    // Whether the source had whitespace immediately before the next token. It
    // carries across run boundaries so abutting runs ("x" + bold "y" = "xy")
    // don't gain a space, while truly separated runs ("a " + "b") keep theirs.
    let mut pending_space = false;

    let flush = |cur: &mut Vec<Piece>, cur_w: &mut f32, lines: &mut Vec<Vec<Piece>>| {
        if !cur.is_empty() {
            lines.push(std::mem::take(cur));
        }
        *cur_w = 0.0;
    };

    for run in runs {
        let bold = base_bold || run.bold;
        let italic = run.italic;
        for (seg_idx, segment) in run.text.split('\n').enumerate() {
            if seg_idx > 0 {
                flush(&mut cur, &mut cur_w, &mut lines); // hard break
                pending_space = false;
            }
            if segment.starts_with(char::is_whitespace) {
                pending_space = true;
            }
            let expanded = segment.replace('\t', "    ");
            let mut first_word = true;
            for word in expanded.split_whitespace() {
                // Words after the first in a segment are whitespace-separated;
                // the first consults the carried boundary state.
                let want_space = if first_word { pending_space } else { true };
                first_word = false;
                for (ci, chunk) in break_word(word, size, bold, max_width)
                    .into_iter()
                    .enumerate()
                {
                    let ww = text_width(&chunk, size, bold);
                    // Only the first chunk of a word can carry the leading space;
                    // char-split continuation chunks never do.
                    let lead = if !cur.is_empty() && want_space && ci == 0 {
                        space_w
                    } else {
                        0.0
                    };
                    if !cur.is_empty() && cur_w + lead + ww > max_width {
                        flush(&mut cur, &mut cur_w, &mut lines);
                        // A leading space is dropped at the start of a line.
                        cur.push(Piece {
                            text: chunk,
                            bold,
                            italic,
                            width: ww,
                            space_before: false,
                        });
                        cur_w = ww;
                    } else {
                        cur.push(Piece {
                            text: chunk,
                            bold,
                            italic,
                            width: ww,
                            space_before: lead > 0.0,
                        });
                        cur_w += lead + ww;
                    }
                }
            }
            // Trailing whitespace (or an all-whitespace segment) separates this
            // run from whatever token comes next.
            pending_space = expanded.ends_with(char::is_whitespace);
        }
    }
    flush(&mut cur, &mut cur_w, &mut lines);
    lines
}

/// Split a word that is wider than `max_width` into character chunks that fit,
/// so a long token (e.g. a URL) can't overflow the right margin. Words that fit
/// are returned unchanged.
fn break_word(word: &str, size: f32, bold: bool, max_width: f32) -> Vec<String> {
    if text_width(word, size, bold) <= max_width {
        return vec![word.to_string()];
    }
    let mut chunks = Vec::new();
    let mut cur = String::new();
    for ch in word.chars() {
        let trial_w = text_width(&format!("{cur}{ch}"), size, bold);
        if !cur.is_empty() && trial_w > max_width {
            chunks.push(std::mem::take(&mut cur));
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// Wrap all paragraphs of a table cell into a flat list of lines.
fn wrap_cell(cell: &Cell, size: f32, max_width: f32) -> Vec<Vec<Piece>> {
    let mut lines = Vec::new();
    for para in &cell.paras {
        lines.extend(wrap(&para.runs, size, false, max_width.max(1.0)));
    }
    lines
}

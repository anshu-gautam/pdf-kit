//! The intermediate document model the parser produces and the layout engine
//! consumes. Deliberately small: a flat list of block-level items, each carrying
//! just enough structure (style, alignment, styled text runs) to lay out a
//! readable PDF. Full WordprocessingML fidelity is a non-goal.

/// A parsed document: an ordered list of block-level items.
#[derive(Debug, Default)]
pub struct Document {
    /// Block-level content in reading order.
    pub blocks: Vec<Block>,
}

/// A block-level item.
#[derive(Debug)]
pub enum Block {
    /// A paragraph (body text, heading, title, or list item).
    Para(Para),
    /// A table (grid of cells).
    Table(Table),
    /// A block-level image (extracted from an inline `w:drawing`).
    Image(Image),
}

/// Horizontal alignment of a paragraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Left-aligned (the default).
    #[default]
    Left,
    /// Centered.
    Center,
    /// Right-aligned.
    Right,
    /// Justified (rendered left-aligned in this version).
    Justify,
}

/// What kind of paragraph this is — drives font size, weight, and spacing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParaKind {
    /// Normal body text.
    Body,
    /// A heading at the given level (1..=6).
    Heading(u8),
    /// A document title.
    Title,
    /// A list item with a precomputed marker (e.g. "• " or "2. ") and a
    /// zero-based indent level.
    ListItem { marker: String, level: u8 },
}

/// A paragraph: a run of styled text spans sharing an alignment and kind.
#[derive(Debug)]
pub struct Para {
    /// Paragraph kind.
    pub kind: ParaKind,
    /// Horizontal alignment.
    pub align: Align,
    /// Styled text spans, in order.
    pub runs: Vec<TextRun>,
}

impl Para {
    /// True if the paragraph has no visible text.
    pub fn is_empty(&self) -> bool {
        self.runs.iter().all(|r| r.text.trim().is_empty())
    }
}

/// A styled span of text within a paragraph.
#[derive(Debug, Clone)]
pub struct TextRun {
    /// The text content.
    pub text: String,
    /// Bold weight.
    pub bold: bool,
    /// Italic style.
    pub italic: bool,
}

/// A table: rows of cells. Ragged rows are tolerated.
#[derive(Debug)]
pub struct Table {
    /// Rows, top to bottom.
    pub rows: Vec<Vec<Cell>>,
}

/// A table cell holding paragraphs.
#[derive(Debug, Default)]
pub struct Cell {
    /// The cell's paragraphs.
    pub paras: Vec<Para>,
}

/// A decoded image ready to place: PNG bytes plus natural pixel size and the
/// requested display size in points (from the drawing's EMU extent, if any).
#[derive(Debug)]
pub struct Image {
    /// PNG-encoded pixels.
    pub png: Vec<u8>,
    /// Natural width in pixels.
    pub px_w: u32,
    /// Natural height in pixels.
    pub px_h: u32,
    /// Requested display width in points, if the drawing specified an extent.
    pub pt_w: Option<f32>,
    /// Requested display height in points, if the drawing specified an extent.
    pub pt_h: Option<f32>,
}

//! The document model: [`Engine`], [`Document`], [`Page`], and [`Metadata`].
//!
//! Public page numbers are **one-based** (PRD invariant). Internally we keep a
//! zero-based vector of page object ids and convert at the boundary.

use lopdf::{Dictionary, Document as LoDoc, Object, ObjectId};

use crate::classify::{self, PageKind, PageSignals};
use crate::error::PdfError;
use crate::textrun::{self, TextRun};
use crate::types::{OpenOptions, PdfInput, TextOptions};

/// Reusable entry point for opening documents.
///
/// `Engine` is intentionally cheap; it exists so callers have a stable handle
/// and so future parser configuration has a home without an API break.
#[derive(Debug, Default, Clone)]
pub struct Engine {
    _private: (),
}

impl Engine {
    /// Create a new engine.
    pub fn new() -> Result<Self, PdfError> {
        Ok(Engine { _private: () })
    }

    /// Open a document from a path or in-memory bytes.
    pub fn open(
        &self,
        input: impl Into<PdfInput>,
        opts: OpenOptions,
    ) -> Result<Document, PdfError> {
        let input = input.into();
        let inner = match (input, opts.password.as_deref()) {
            (PdfInput::Path(p), Some(pw)) => LoDoc::load_with_password(&p, pw),
            (PdfInput::Path(p), None) => LoDoc::load(&p),
            (PdfInput::Bytes(b), Some(pw)) => LoDoc::load_mem_with_password(&b, pw),
            (PdfInput::Bytes(b), None) => LoDoc::load_mem(&b),
        }
        .map_err(PdfError::from)?;
        // lopdf loads an encrypted document even when the password is missing,
        // leaving it locked (not decrypted). A wrong password fails during load,
        // but a *missing* one does not — so reject the still-locked case here.
        if inner.is_encrypted() && !inner.was_encrypted() {
            return Err(PdfError::Password);
        }
        Ok(Document::from_lopdf(inner))
    }
}

/// Document-level metadata (PRD §4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// `/Title` from the document information dictionary, if present.
    pub title: Option<String>,
    /// `/Author` from the document information dictionary, if present.
    pub author: Option<String>,
    /// Number of pages.
    pub page_count: usize,
    /// The PDF version string from the header (e.g. `"1.7"`).
    pub pdf_version: String,
    /// Whether the document was encrypted when it was opened.
    pub encrypted: bool,
}

/// An opened PDF document. Owns the parsed `lopdf` document.
#[derive(Debug)]
pub struct Document {
    inner: LoDoc,
    /// Page object ids, index 0 == one-based page 1, in page order.
    page_ids: Vec<ObjectId>,
    metadata: Metadata,
}

impl Document {
    fn from_lopdf(inner: LoDoc) -> Document {
        // `get_pages` is a BTreeMap keyed by one-based page number, so iterating
        // its values yields object ids already in page order.
        let page_ids: Vec<ObjectId> = inner.get_pages().into_values().collect();
        let metadata = build_metadata(&inner, page_ids.len());
        Document {
            inner,
            page_ids,
            metadata,
        }
    }

    /// Document metadata, computed once at open time.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Number of pages in the document.
    pub fn page_count(&self) -> usize {
        self.page_ids.len()
    }

    /// Access a single page by its **one-based** number.
    pub fn page(&self, one_based: usize) -> Result<Page<'_>, PdfError> {
        let id = self
            .page_ids
            .get(one_based.wrapping_sub(1))
            .filter(|_| one_based >= 1)
            .copied()
            .ok_or(PdfError::PageRange(one_based))?;
        Ok(Page {
            doc: self,
            number: one_based,
            id,
        })
    }

    /// Iterate over all pages in order.
    pub fn pages(&self) -> impl Iterator<Item = Page<'_>> + '_ {
        self.page_ids.iter().enumerate().map(move |(i, &id)| Page {
            doc: self,
            number: i + 1,
            id,
        })
    }

    /// Extract text across the document, honoring [`TextOptions`].
    pub fn text(&self, opts: TextOptions) -> Result<String, PdfError> {
        let selected = self.select_pages(opts.pages.as_deref(), opts.max_pages)?;
        let mut out = String::new();
        let mut count = 0usize;
        for (i, &page_no) in selected.iter().enumerate() {
            if count >= opts.max_chars {
                break;
            }
            let piece = self.extract_page_text(page_no)?;
            if i > 0 && !out.is_empty() {
                out.push('\n');
                count += 1;
            }
            push_capped(&mut out, &piece, &mut count, opts.max_chars);
        }
        Ok(out)
    }

    /// Resolve the ordered list of one-based page numbers to visit.
    fn select_pages(
        &self,
        pages: Option<&[usize]>,
        max_pages: usize,
    ) -> Result<Vec<usize>, PdfError> {
        let count = self.page_count();
        let selected: Vec<usize> = match pages {
            Some(list) => {
                for &p in list {
                    if p == 0 || p > count {
                        return Err(PdfError::PageRange(p));
                    }
                }
                list.to_vec()
            }
            None => (1..=count).collect(),
        };
        Ok(selected.into_iter().take(max_pages).collect())
    }

    fn extract_page_text(&self, one_based: usize) -> Result<String, PdfError> {
        let id = self
            .page_ids
            .get(one_based.wrapping_sub(1))
            .filter(|_| one_based >= 1)
            .copied()
            .ok_or(PdfError::PageRange(one_based))?;
        // Layout-aware reflow (positioned, encoding-correct) rather than lopdf's
        // fragment-per-operation extract_text, so paragraphs read naturally.
        Ok(textrun::page_text(&self.inner, id))
    }

    /// Look up an attribute on a page, walking `/Parent` for inheritance
    /// (MediaBox, Rotate, Resources are inheritable per the PDF spec).
    fn inherited_resolved(&self, page_id: ObjectId, key: &[u8]) -> Option<Object> {
        let mut id = page_id;
        for _ in 0..32 {
            let dict = self.inner.get_dictionary(id).ok()?;
            if let Ok(obj) = dict.get(key) {
                return Some(self.resolve(obj).clone());
            }
            id = dict.get(b"Parent").ok()?.as_reference().ok()?;
        }
        None
    }

    /// Follow a single indirect reference, if the object is one.
    fn resolve<'a>(&'a self, obj: &'a Object) -> &'a Object {
        match obj.as_reference() {
            Ok(id) => self.inner.get_object(id).unwrap_or(obj),
            Err(_) => obj,
        }
    }
}

/// A borrowed view of a single page.
pub struct Page<'d> {
    doc: &'d Document,
    number: usize,
    id: ObjectId,
}

impl Page<'_> {
    /// The one-based page number.
    pub fn number(&self) -> usize {
        self.number
    }

    /// `(width, height)` in PostScript points, from the (inherited) MediaBox.
    /// Defaults to US Letter (612 × 792) if no MediaBox can be found.
    pub fn size_points(&self) -> (f32, f32) {
        if let Some(obj) = self.doc.inherited_resolved(self.id, b"MediaBox") {
            if let Ok(arr) = obj.as_array() {
                if arr.len() == 4 {
                    let n = |o: &Object| o.as_float().unwrap_or(0.0);
                    let (x0, y0, x1, y1) = (n(&arr[0]), n(&arr[1]), n(&arr[2]), n(&arr[3]));
                    return ((x1 - x0).abs(), (y1 - y0).abs());
                }
            }
        }
        (612.0, 792.0)
    }

    /// Page rotation in degrees (normalized to 0/90/180/270), default 0.
    pub fn rotation(&self) -> i32 {
        let deg = self
            .doc
            .inherited_resolved(self.id, b"Rotate")
            .and_then(|o| o.as_i64().ok())
            .unwrap_or(0);
        ((deg % 360 + 360) % 360) as i32
    }

    /// Extract the text on this page from its text layer.
    pub fn text(&self) -> Result<String, PdfError> {
        self.doc.extract_page_text(self.number)
    }

    /// Raw classification signals for this page (text char count, image count,
    /// image coverage). Exposed so callers can apply their own thresholds.
    pub fn signals(&self) -> PageSignals {
        let (w, h) = self.size_points();
        // One content decode for both text-char count and image coverage; no
        // reflow (we only need the count, not laid-out text).
        classify::page_signals(&self.doc.inner, self.id, w, h)
    }

    /// Classify this page (text-based / scanned / image-only / mixed).
    pub fn classify(&self) -> PageKind {
        classify::classify(&self.signals())
    }

    /// Positioned text runs on this page (text, bounding box, effective font
    /// size), in content-stream order. Used by chunking to detect headings and
    /// group blocks.
    pub fn text_runs(&self) -> Vec<TextRun> {
        textrun::page_text_runs(&self.doc.inner, self.id)
    }
}

#[cfg(feature = "render-native")]
impl Page<'_> {
    /// Crate-internal handle for the native renderer: the parsed document and
    /// this page's object id.
    pub(crate) fn render_handle(&self) -> (&LoDoc, ObjectId) {
        (&self.doc.inner, self.id)
    }
}

fn build_metadata(doc: &LoDoc, page_count: usize) -> Metadata {
    let (title, author) = info_strings(doc);
    Metadata {
        title,
        author,
        page_count,
        pdf_version: doc.version.clone(),
        encrypted: doc.was_encrypted() || doc.is_encrypted(),
    }
}

/// Read `/Title` and `/Author` from the document information dictionary.
fn info_strings(doc: &LoDoc) -> (Option<String>, Option<String>) {
    let info: Option<&Dictionary> = doc.trailer.get(b"Info").ok().and_then(|o| {
        if let Ok(id) = o.as_reference() {
            doc.get_dictionary(id).ok()
        } else {
            o.as_dict().ok()
        }
    });
    let read = |key: &[u8]| -> Option<String> {
        info.and_then(|d| d.get(key).ok())
            .and_then(|o| o.as_str().ok())
            .map(decode_pdf_text)
    };
    (read(b"Title"), read(b"Author"))
}

/// Decode a PDF text string: UTF-16BE when it carries a BOM, otherwise treat the
/// bytes as Latin-1 (a close-enough subset of PDFDocEncoding for v1).
// TODO(design): implement the full PDFDocEncoding table for the non-BOM case.
fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

/// Append `piece` to `out`, stopping at `max_chars` total characters.
fn push_capped(out: &mut String, piece: &str, count: &mut usize, max_chars: usize) {
    for ch in piece.chars() {
        if *count >= max_chars {
            break;
        }
        out.push(ch);
        *count += 1;
    }
}

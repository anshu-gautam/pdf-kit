//! The document model: [`Engine`], [`Document`], [`Page`], and [`Metadata`].
//!
//! Public page numbers are **one-based** (PRD invariant). Internally we keep a
//! zero-based vector of page object ids and convert at the boundary.

use std::collections::HashSet;

use lopdf::{Dictionary, Document as LoDoc, Object, ObjectId};

use crate::classify::{self, PageKind, PageSignals};
use crate::error::PdfError;
use crate::tagged::{self, StructNode};
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

/// Document-level metadata (PRD §4.1), from the information dictionary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// `/Title`, if present.
    pub title: Option<String>,
    /// `/Author`, if present.
    pub author: Option<String>,
    /// `/Subject`, if present.
    pub subject: Option<String>,
    /// `/Keywords`, if present.
    pub keywords: Option<String>,
    /// `/Creator` (the authoring app), if present.
    pub creator: Option<String>,
    /// `/Producer` (the PDF library), if present.
    pub producer: Option<String>,
    /// `/CreationDate`, raw PDF date string (e.g. `"D:20240101120000Z"`).
    pub creation_date: Option<String>,
    /// `/ModDate`, raw PDF date string.
    pub mod_date: Option<String>,
    /// Number of pages.
    pub page_count: usize,
    /// The PDF version string from the header (e.g. `"1.7"`).
    pub pdf_version: String,
    /// Whether the document was encrypted when it was opened.
    pub encrypted: bool,
}

/// A bookmark / table-of-contents entry. One-based `page` is `None` when the
/// destination can't be resolved to a page in this document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    /// The bookmark label.
    pub title: String,
    /// The one-based page the bookmark points at, if resolvable.
    pub page: Option<usize>,
    /// Nested child bookmarks.
    pub children: Vec<OutlineItem>,
}

/// Where a link annotation points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// An external URI.
    Uri(String),
    /// A one-based page in this document.
    Page(usize),
}

/// A link annotation on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    /// Clickable rectangle `[x0, y0, x1, y1]` in points (normalized so
    /// `x0 <= x1`, `y0 <= y1`).
    pub rect: [f32; 4],
    /// The link destination.
    pub target: LinkTarget,
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

    /// The tagged-PDF logical structure tree ([`StructNode`]), or `None` when the
    /// document is not tagged (`/MarkInfo /Marked true` + a `/StructTreeRoot`).
    /// When present this is authoritative structure — heading levels, table
    /// cells, list nesting, figure alt-text, and reading order.
    pub fn structure_tree(&self) -> Option<StructNode> {
        tagged::structure_tree(&self.inner, &self.page_ids)
    }

    /// The document outline (bookmarks / table of contents) as a tree, in order.
    /// Empty when the document has none. Best-effort: an entry whose destination
    /// can't be resolved still appears, with `page == None`.
    pub fn outline(&self) -> Vec<OutlineItem> {
        let Ok(catalog) = self.inner.catalog() else {
            return Vec::new();
        };
        let first = catalog
            .get(b"Outlines")
            .ok()
            .and_then(|o| o.as_reference().ok())
            .and_then(|id| self.inner.get_dictionary(id).ok())
            .and_then(|root| root.get(b"First").ok())
            .and_then(|o| o.as_reference().ok());
        let mut visited = HashSet::new();
        self.outline_siblings(first, &mut visited, 0)
    }

    /// Walk a `/First`-then-`/Next` sibling chain into [`OutlineItem`]s, recursing
    /// into `/First` for children. The shared `visited` set and depth cap make a
    /// malformed (cyclic) outline terminate.
    fn outline_siblings(
        &self,
        mut next: Option<ObjectId>,
        visited: &mut HashSet<ObjectId>,
        depth: usize,
    ) -> Vec<OutlineItem> {
        let mut items = Vec::new();
        if depth > 32 {
            return items;
        }
        while let Some(id) = next {
            if !visited.insert(id) {
                break; // cycle
            }
            let Ok(dict) = self.inner.get_dictionary(id) else {
                break;
            };
            let title = self.dict_text(dict, b"Title").unwrap_or_default();
            let page = self.resolve_dest_page(dict);
            let child_first = dict.get(b"First").ok().and_then(|o| o.as_reference().ok());
            let children = self.outline_siblings(child_first, visited, depth + 1);
            items.push(OutlineItem {
                title,
                page,
                children,
            });
            next = dict.get(b"Next").ok().and_then(|o| o.as_reference().ok());
        }
        items
    }

    /// Resolve an outline/link dict's destination (`/Dest`, or a `GoTo` `/A`) to
    /// a one-based page number.
    fn resolve_dest_page(&self, dict: &Dictionary) -> Option<usize> {
        let dest: &Object = if let Ok(d) = dict.get(b"Dest") {
            self.resolve(d)
        } else {
            let action = dict.get(b"A").ok().and_then(|o| self.deref_dict(o))?;
            let is_goto = action
                .get(b"S")
                .ok()
                .and_then(|o| o.as_name().ok())
                .map(|s| s == b"GoTo")
                .unwrap_or(false);
            if !is_goto {
                return None;
            }
            self.resolve(action.get(b"D").ok()?)
        };
        let page_id = self.dest_page_id(dest)?;
        self.page_number_of(page_id)
    }

    /// The page object id a (resolved) destination object points at.
    fn dest_page_id(&self, dest: &Object) -> Option<ObjectId> {
        match dest {
            Object::Array(arr) => arr.first()?.as_reference().ok(),
            Object::Name(n) => self.resolve_named_dest(n),
            Object::String(s, _) => self.resolve_named_dest(s),
            _ => None,
        }
    }

    /// Resolve a named destination via the catalog's old-style `/Dests` dict or
    /// the `/Names` -> `/Dests` name tree.
    fn resolve_named_dest(&self, name: &[u8]) -> Option<ObjectId> {
        let catalog = self.inner.catalog().ok()?;
        if let Some(dests) = catalog.get(b"Dests").ok().and_then(|o| self.deref_dict(o)) {
            if let Ok(value) = dests.get(name) {
                if let Some(id) = self.dest_value_page_id(self.resolve(value)) {
                    return Some(id);
                }
            }
        }
        let names = catalog
            .get(b"Names")
            .ok()
            .and_then(|o| self.deref_dict(o))?;
        let tree = names.get(b"Dests").ok().and_then(|o| self.deref_dict(o))?;
        let mut visited = HashSet::new();
        self.search_name_tree(tree, name, &mut visited, 0)
    }

    /// A named-destination value is either a destination array or a dict with a
    /// `/D` destination array; either way, return its page object id.
    fn dest_value_page_id(&self, value: &Object) -> Option<ObjectId> {
        match value {
            Object::Array(arr) => arr.first()?.as_reference().ok(),
            Object::Dictionary(d) => match self.resolve(d.get(b"D").ok()?) {
                Object::Array(arr) => arr.first()?.as_reference().ok(),
                _ => None,
            },
            _ => None,
        }
    }

    /// Search a destination name tree (`/Kids` intermediate nodes, `/Names`
    /// `[key value ...]` leaves) for `target`. Depth-bounded.
    fn search_name_tree(
        &self,
        node: &Dictionary,
        target: &[u8],
        visited: &mut HashSet<ObjectId>,
        depth: usize,
    ) -> Option<ObjectId> {
        if depth > 32 {
            return None;
        }
        if let Some(names) = node.get(b"Names").ok().and_then(|o| self.deref_array(o)) {
            let mut i = 0;
            while i + 1 < names.len() {
                if names[i].as_str().is_ok_and(|k| k == target) {
                    return self.dest_value_page_id(self.resolve(&names[i + 1]));
                }
                i += 2;
            }
        }
        if let Some(kids) = node.get(b"Kids").ok().and_then(|o| self.deref_array(o)) {
            for kid in kids {
                let Ok(kid_id) = kid.as_reference() else {
                    continue;
                };
                // Visit each node once, so a cyclic /Kids terminates immediately
                // (the depth cap is a secondary bound).
                if !visited.insert(kid_id) {
                    continue;
                }
                if let Ok(kid_dict) = self.inner.get_dictionary(kid_id) {
                    if let Some(found) = self.search_name_tree(kid_dict, target, visited, depth + 1)
                    {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    /// The link annotations on a page object, in order. Best-effort; empty when
    /// the page has no `/Annots`.
    fn page_links(&self, page_id: ObjectId) -> Vec<Link> {
        let Ok(annots) = self.inner.get_page_annotations(page_id) else {
            return Vec::new();
        };
        let mut links = Vec::new();
        for annot in annots {
            let is_link = annot
                .get(b"Subtype")
                .ok()
                .and_then(|o| o.as_name().ok())
                .map(|s| s == b"Link")
                .unwrap_or(false);
            if !is_link {
                continue;
            }
            let (Some(rect), Some(target)) = (self.rect_of(annot), self.link_target(annot)) else {
                continue;
            };
            links.push(Link { rect, target });
        }
        links
    }

    /// A link annotation's target: an external URI (`/A` `/S /URI`) or an
    /// internal page (`/Dest` or a `GoTo` `/A`).
    fn link_target(&self, annot: &Dictionary) -> Option<LinkTarget> {
        if let Some(action) = annot.get(b"A").ok().and_then(|o| self.deref_dict(o)) {
            let is_uri = action
                .get(b"S")
                .ok()
                .and_then(|o| o.as_name().ok())
                .map(|s| s == b"URI")
                .unwrap_or(false);
            if is_uri {
                let uri = action.get(b"URI").ok().and_then(|o| o.as_str().ok())?;
                return Some(LinkTarget::Uri(String::from_utf8_lossy(uri).into_owned()));
            }
        }
        self.resolve_dest_page(annot).map(LinkTarget::Page)
    }

    /// A normalized `/Rect` `[x0, y0, x1, y1]` (so `x0 <= x1`, `y0 <= y1`).
    fn rect_of(&self, annot: &Dictionary) -> Option<[f32; 4]> {
        let arr = annot.get(b"Rect").ok().and_then(|o| self.deref_array(o))?;
        if arr.len() < 4 {
            return None;
        }
        let n = |i: usize| self.resolve(&arr[i]).as_float().ok();
        let (a, b, c, d) = (n(0)?, n(1)?, n(2)?, n(3)?);
        Some([a.min(c), b.min(d), a.max(c), b.max(d)])
    }

    /// One-based page number of a page object id, if it is one of our pages.
    fn page_number_of(&self, id: ObjectId) -> Option<usize> {
        self.page_ids.iter().position(|&p| p == id).map(|i| i + 1)
    }

    /// Read a (possibly indirect) text-string field and decode it.
    fn dict_text(&self, dict: &Dictionary, key: &[u8]) -> Option<String> {
        dict.get(key)
            .ok()
            .map(|o| self.resolve(o))
            .and_then(|o| o.as_str().ok())
            .map(decode_pdf_text)
    }

    /// Dereference one level to a dictionary.
    fn deref_dict<'a>(&'a self, obj: &'a Object) -> Option<&'a Dictionary> {
        match obj.as_reference() {
            Ok(id) => self.inner.get_dictionary(id).ok(),
            Err(_) => obj.as_dict().ok(),
        }
    }

    /// Dereference one level to an array.
    fn deref_array<'a>(&'a self, obj: &'a Object) -> Option<&'a Vec<Object>> {
        match obj.as_reference() {
            Ok(id) => self
                .inner
                .get_object(id)
                .ok()
                .and_then(|o| o.as_array().ok()),
            Err(_) => obj.as_array().ok(),
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

    /// Link annotations on this page (clickable rect + URI/internal-page target).
    pub fn links(&self) -> Vec<Link> {
        self.doc.page_links(self.id)
    }

    /// Image / figure regions painted on this page: each image XObject's bounding
    /// box (in points) plus its nearest caption line, if any. Render the page and
    /// [`crate::Bitmap::crop_region`] a bbox to extract that figure as an image.
    pub fn image_regions(&self) -> Vec<crate::figures::ImageRegion> {
        crate::figures::image_regions(&self.doc.inner, self.id)
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
    Metadata {
        title: read(b"Title"),
        author: read(b"Author"),
        subject: read(b"Subject"),
        keywords: read(b"Keywords"),
        creator: read(b"Creator"),
        producer: read(b"Producer"),
        creation_date: read(b"CreationDate"),
        mod_date: read(b"ModDate"),
        page_count,
        pdf_version: doc.version.clone(),
        encrypted: doc.was_encrypted() || doc.is_encrypted(),
    }
}

/// Decode a PDF text string: UTF-16BE when it carries a BOM, otherwise treat the
/// bytes as Latin-1 (a close-enough subset of PDFDocEncoding for v1).
// TODO(design): implement the full PDFDocEncoding table for the non-BOM case.
fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let pairs = bytes[2..].chunks_exact(2);
        let remainder = pairs.remainder();
        let mut units: Vec<u16> = pairs.map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        // A trailing odd byte is malformed; surface it as U+FFFD rather than
        // silently dropping it.
        if !remainder.is_empty() {
            units.push(0xFFFD);
        }
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

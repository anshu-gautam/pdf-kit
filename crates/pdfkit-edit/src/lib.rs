//! `pdfkit-edit` — the write path (create + edit), built on lopdf's object model
//! (PRD §4.5). Depends only on `pdfkit-core` for the shared input/error types;
//! it never flows through the extraction engine.

use std::collections::{HashMap, HashSet};
use std::io::Write;

use lopdf::content::{Content, Operation};
use lopdf::{
    dictionary, Dictionary, Document, Object, ObjectId, SaveOptions, Stream, StringFormat,
};

use pdfkit_core::{PdfError, PdfInput};

/// A standard page size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageSize {
    /// 612 × 792 pt.
    Letter,
    /// 595 × 842 pt.
    A4,
    /// 612 × 1008 pt.
    Legal,
    /// Custom size in points.
    Custom { width: f32, height: f32 },
}

impl PageSize {
    fn dims(self) -> (f32, f32) {
        match self {
            PageSize::Letter => (612.0, 792.0),
            PageSize::A4 => (595.0, 842.0),
            PageSize::Legal => (612.0, 1008.0),
            PageSize::Custom { width, height } => (width, height),
        }
    }
}

/// One of the standard-14 font families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFamily {
    /// Helvetica.
    Helvetica,
    /// Times-Roman.
    TimesRoman,
    /// Courier.
    Courier,
}

/// Font selection for [`PdfBuilder::draw_text`].
#[derive(Debug, Clone, Copy)]
pub struct FontSpec {
    /// Font family.
    pub family: FontFamily,
    /// Size in points.
    pub size: f32,
    /// Bold weight (selects the `-Bold` standard-14 variant).
    pub bold: bool,
    /// Italic/oblique style (selects the `-Italic` / `-Oblique` variant).
    pub italic: bool,
}

impl Default for FontSpec {
    fn default() -> Self {
        FontSpec {
            family: FontFamily::Helvetica,
            size: 12.0,
            bold: false,
            italic: false,
        }
    }
}

impl FontSpec {
    /// The standard-14 BaseFont name for this family + style combination.
    fn base_font(self) -> &'static str {
        use FontFamily::*;
        match (self.family, self.bold, self.italic) {
            (Helvetica, false, false) => "Helvetica",
            (Helvetica, true, false) => "Helvetica-Bold",
            (Helvetica, false, true) => "Helvetica-Oblique",
            (Helvetica, true, true) => "Helvetica-BoldOblique",
            (TimesRoman, false, false) => "Times-Roman",
            (TimesRoman, true, false) => "Times-Bold",
            (TimesRoman, false, true) => "Times-Italic",
            (TimesRoman, true, true) => "Times-BoldItalic",
            (Courier, false, false) => "Courier",
            (Courier, true, false) => "Courier-Bold",
            (Courier, false, true) => "Courier-Oblique",
            (Courier, true, true) => "Courier-BoldOblique",
        }
    }
}

/// Encode a `str` as WinAnsi (CP1252) bytes for a PDF literal string. ASCII is
/// passed through; the Latin-1 upper range maps 1:1; a handful of common CP1252
/// punctuation (smart quotes, dashes, ellipsis, bullet, €, ™) is mapped to its
/// byte; anything else becomes `?`. Paired with `/Encoding /WinAnsiEncoding` on
/// the font, this renders typical Word/office text correctly.
fn encode_winansi(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for ch in s.chars() {
        let byte = match ch {
            '\u{20}'..='\u{7e}' => ch as u8,
            // Latin-1 supplement maps 1:1 onto WinAnsi 0xA0..=0xFF.
            '\u{a0}'..='\u{ff}' => ch as u8,
            // CP1252 punctuation block (0x80..=0x9F).
            '\u{20ac}' => 0x80, // €
            '\u{201a}' => 0x82, // ‚
            '\u{0192}' => 0x83, // ƒ
            '\u{201e}' => 0x84, // „
            '\u{2026}' => 0x85, // …
            '\u{2020}' => 0x86, // †
            '\u{2021}' => 0x87, // ‡
            '\u{02c6}' => 0x88, // ˆ
            '\u{2030}' => 0x89, // ‰
            '\u{0160}' => 0x8a, // Š
            '\u{2039}' => 0x8b, // ‹
            '\u{0152}' => 0x8c, // Œ
            '\u{017d}' => 0x8e, // Ž
            '\u{2018}' => 0x91, // ‘
            '\u{2019}' => 0x92, // ’
            '\u{201c}' => 0x93, // “
            '\u{201d}' => 0x94, // ”
            '\u{2022}' => 0x95, // •
            '\u{2013}' => 0x96, // –
            '\u{2014}' => 0x97, // —
            '\u{02dc}' => 0x98, // ˜
            '\u{2122}' => 0x99, // ™
            '\u{0161}' => 0x9a, // š
            '\u{203a}' => 0x9b, // ›
            '\u{0153}' => 0x9c, // œ
            '\u{017e}' => 0x9e, // ž
            '\u{0178}' => 0x9f, // Ÿ
            '\t' => b' ',
            _ => b'?',
        };
        out.push(byte);
    }
    out
}

/// Opaque handle to a page being authored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRef(usize);

struct PlacedImage {
    name: String,
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

#[derive(Default)]
struct BuildPage {
    width: f32,
    height: f32,
    ops: Vec<Operation>,
    fonts: Vec<(&'static str, String)>, // (base font, resource name)
    images: Vec<PlacedImage>,
}

impl BuildPage {
    /// Resource name for a base font on this page, creating it if needed.
    fn font_name(&mut self, base: &'static str) -> String {
        if let Some((_, name)) = self.fonts.iter().find(|(b, _)| *b == base) {
            return name.clone();
        }
        let name = format!("F{}", self.fonts.len() + 1);
        self.fonts.push((base, name.clone()));
        name
    }
}

/// Author a new PDF document.
#[derive(Default)]
pub struct PdfBuilder {
    pages: Vec<BuildPage>,
}

impl PdfBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        PdfBuilder::default()
    }

    /// Add a page and return a handle to it.
    pub fn add_page(&mut self, size: PageSize) -> PageRef {
        let (width, height) = size.dims();
        self.pages.push(BuildPage {
            width,
            height,
            ..BuildPage::default()
        });
        PageRef(self.pages.len() - 1)
    }

    /// Draw `text` at `at` (points, origin bottom-left) in `font`.
    pub fn draw_text(&mut self, page: PageRef, text: &str, at: (f32, f32), font: FontSpec) {
        let Some(p) = self.pages.get_mut(page.0) else {
            return;
        };
        let name = p.font_name(font.base_font());
        p.ops.push(Operation::new("BT", vec![]));
        p.ops.push(Operation::new(
            "Tf",
            vec![name.as_str().into(), font.size.into()],
        ));
        p.ops.push(Operation::new(
            "Tm",
            vec![
                1.0f32.into(),
                0.0f32.into(),
                0.0f32.into(),
                1.0f32.into(),
                at.0.into(),
                at.1.into(),
            ],
        ));
        p.ops.push(Operation::new(
            "Tj",
            vec![Object::String(encode_winansi(text), StringFormat::Literal)],
        ));
        p.ops.push(Operation::new("ET", vec![]));
    }

    /// Stroke a straight line from `from` to `to` (points, origin bottom-left)
    /// at `width` pt in `gray` (0.0 black ..= 1.0 white). Used for rules and
    /// simple table borders on authored pages.
    pub fn draw_line(
        &mut self,
        page: PageRef,
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
        gray: f32,
    ) {
        let Some(p) = self.pages.get_mut(page.0) else {
            return;
        };
        p.ops.push(Operation::new("q", vec![]));
        p.ops.push(Operation::new("w", vec![width.into()]));
        p.ops
            .push(Operation::new("G", vec![gray.clamp(0.0, 1.0).into()]));
        p.ops
            .push(Operation::new("m", vec![from.0.into(), from.1.into()]));
        p.ops
            .push(Operation::new("l", vec![to.0.into(), to.1.into()]));
        p.ops.push(Operation::new("S", vec![]));
        p.ops.push(Operation::new("Q", vec![]));
    }

    /// Place a PNG image into `rect` (`[x0, y0, x1, y1]` in points).
    pub fn place_image(
        &mut self,
        page: PageRef,
        png: &[u8],
        rect: [f32; 4],
    ) -> Result<(), PdfError> {
        let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .map_err(|e| PdfError::Backend(format!("decode png: {e}")))?
            .to_rgb8();
        let (width, height) = (img.width(), img.height());

        let Some(p) = self.pages.get_mut(page.0) else {
            return Err(PdfError::PageRange(page.0 + 1));
        };
        let name = format!("Im{}", p.images.len() + 1);
        p.images.push(PlacedImage {
            name: name.clone(),
            width,
            height,
            rgb: img.into_raw(),
        });

        let (x0, y0, x1, y1) = (rect[0], rect[1], rect[2], rect[3]);
        p.ops.push(Operation::new("q", vec![]));
        p.ops.push(Operation::new(
            "cm",
            vec![
                (x1 - x0).into(),
                0.0f32.into(),
                0.0f32.into(),
                (y1 - y0).into(),
                x0.into(),
                y0.into(),
            ],
        ));
        p.ops.push(Operation::new("Do", vec![name.as_str().into()]));
        p.ops.push(Operation::new("Q", vec![]));
        Ok(())
    }

    /// Assemble the document.
    fn assemble(&self) -> Result<Document, PdfError> {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let mut kids: Vec<Object> = Vec::new();

        for page in &self.pages {
            let mut resources = Dictionary::new();

            if !page.fonts.is_empty() {
                let mut fonts = Dictionary::new();
                for (base, name) in &page.fonts {
                    let font_id = doc.add_object(dictionary! {
                        "Type" => "Font",
                        "Subtype" => "Type1",
                        "BaseFont" => *base,
                        "Encoding" => "WinAnsiEncoding",
                    });
                    fonts.set(name.as_str(), font_id);
                }
                resources.set("Font", fonts);
            }

            if !page.images.is_empty() {
                let mut xobjects = Dictionary::new();
                for img in &page.images {
                    let stream = Stream::new(
                        dictionary! {
                            "Type" => "XObject",
                            "Subtype" => "Image",
                            "Width" => img.width as i64,
                            "Height" => img.height as i64,
                            "ColorSpace" => "DeviceRGB",
                            "BitsPerComponent" => 8_i64,
                        },
                        img.rgb.clone(),
                    );
                    let id = doc.add_object(stream);
                    xobjects.set(img.name.as_str(), id);
                }
                resources.set("XObject", xobjects);
            }

            let resources_id = doc.add_object(resources);
            let content = Content {
                operations: page.ops.clone(),
            };
            let content_id = doc.add_object(Stream::new(
                dictionary! {},
                content
                    .encode()
                    .map_err(|e| PdfError::Backend(format!("encode content: {e}")))?,
            ));

            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.0f32.into(), 0.0f32.into(), page.width.into(), page.height.into()],
                "Contents" => content_id,
                "Resources" => resources_id,
            });
            kids.push(page_id.into());
        }

        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => count,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        Ok(doc)
    }

    /// Serialize the document.
    pub fn save(&self, mut out: impl Write) -> Result<(), PdfError> {
        let mut doc = self.assemble()?;
        doc.save_to(&mut out).map_err(PdfError::from)?;
        Ok(())
    }
}

/// Options for [`PdfEditor::watermark`].
#[derive(Debug, Clone)]
pub struct WatermarkOptions {
    /// Font size in points.
    pub font_size: f32,
    /// Gray level 0.0 (black) ..= 1.0 (white) for the watermark text.
    pub gray: f32,
    /// Rotation in degrees (counter-clockwise).
    pub rotation_degrees: f32,
}

impl Default for WatermarkOptions {
    fn default() -> Self {
        WatermarkOptions {
            font_size: 48.0,
            gray: 0.75,
            rotation_degrees: 45.0,
        }
    }
}

/// Edit an existing PDF document.
pub struct PdfEditor {
    doc: Document,
}

impl PdfEditor {
    /// Open a document from a path or bytes.
    pub fn open(input: impl Into<PdfInput>) -> Result<Self, PdfError> {
        let doc = match input.into() {
            PdfInput::Path(p) => Document::load(&p),
            PdfInput::Bytes(b) => Document::load_mem(&b),
        }
        .map_err(PdfError::from)?;
        Ok(PdfEditor { doc })
    }

    /// Number of pages.
    pub fn page_count(&self) -> usize {
        self.doc.get_pages().len()
    }

    /// Append all pages of `other` after this document's pages.
    pub fn merge(&mut self, other: &PdfEditor) -> Result<(), PdfError> {
        let mut other = other.doc.clone();
        other.renumber_objects_with(self.doc.max_id + 1);

        let pages_root = self.pages_root()?;
        let imported: Vec<ObjectId> = other.get_pages().into_values().collect();

        for (id, obj) in other.objects.clone() {
            self.doc.objects.insert(id, obj);
        }
        self.doc.max_id = self.doc.max_id.max(other.max_id);

        for page_id in &imported {
            if let Ok(dict) = self.doc.get_dictionary_mut(*page_id) {
                dict.set("Parent", pages_root);
            }
        }

        if let Ok(pages) = self.doc.get_dictionary_mut(pages_root) {
            let mut kids = pages
                .get(b"Kids")
                .and_then(Object::as_array)
                .cloned()
                .unwrap_or_default();
            kids.extend(imported.iter().map(|id| Object::Reference(*id)));
            let count = kids.len() as i64;
            pages.set("Kids", kids);
            pages.set("Count", count);
        }
        Ok(())
    }

    /// Split into separate documents, one per inclusive one-based page range.
    pub fn split(&self, ranges: &[(usize, usize)]) -> Result<Vec<Vec<u8>>, PdfError> {
        let total = self.page_count();
        let mut outputs = Vec::with_capacity(ranges.len());
        for &(start, end) in ranges {
            if start == 0 || start > end || end > total {
                return Err(PdfError::PageRange(if start == 0 { start } else { end }));
            }
            let keep: HashSet<u32> = (start as u32..=end as u32).collect();
            let drop: Vec<u32> = (1..=total as u32).filter(|p| !keep.contains(p)).collect();

            let mut doc = self.doc.clone();
            doc.delete_pages(&drop);
            clean_page_tree(&mut doc);
            doc.prune_objects();

            let mut buf = Vec::new();
            doc.save_to(&mut buf).map_err(PdfError::from)?;
            outputs.push(buf);
        }
        Ok(outputs)
    }

    /// Remove the given one-based pages.
    pub fn remove_pages(&mut self, pages: &[usize]) -> Result<(), PdfError> {
        let total = self.page_count();
        for &p in pages {
            if p == 0 || p > total {
                return Err(PdfError::PageRange(p));
            }
        }
        let nums: Vec<u32> = pages.iter().map(|&p| p as u32).collect();
        self.doc.delete_pages(&nums);
        clean_page_tree(&mut self.doc);
        Ok(())
    }

    /// Rotate a one-based page by a multiple of 90 degrees.
    pub fn rotate_page(&mut self, page: usize, degrees: i32) -> Result<(), PdfError> {
        let id = self
            .doc
            .get_pages()
            .get(&(page as u32))
            .copied()
            .ok_or(PdfError::PageRange(page))?;
        let normalized = degrees.rem_euclid(360);
        let dict = self.doc.get_dictionary_mut(id)?;
        dict.set("Rotate", normalized as i64);
        Ok(())
    }

    /// Overlay `text` as a watermark on every page.
    pub fn watermark(&mut self, text: &str, opts: WatermarkOptions) -> Result<(), PdfError> {
        let page_ids: Vec<ObjectId> = self.doc.get_pages().into_values().collect();
        for page_id in page_ids {
            // Ensure a watermark font in the page resources.
            self.ensure_watermark_font(page_id)?;

            let theta = opts.rotation_degrees.to_radians();
            let (cos, sin) = (theta.cos(), theta.sin());
            let content = Content {
                operations: vec![
                    Operation::new("q", vec![]),
                    Operation::new(
                        "rg",
                        vec![opts.gray.into(), opts.gray.into(), opts.gray.into()],
                    ),
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["PDFKitWM".into(), opts.font_size.into()]),
                    Operation::new(
                        "Tm",
                        vec![
                            cos.into(),
                            sin.into(),
                            (-sin).into(),
                            cos.into(),
                            120.0f32.into(),
                            400.0f32.into(),
                        ],
                    ),
                    Operation::new("Tj", vec![Object::string_literal(text)]),
                    Operation::new("ET", vec![]),
                    Operation::new("Q", vec![]),
                ],
            };
            let bytes = content
                .encode()
                .map_err(|e| PdfError::Backend(format!("encode watermark: {e}")))?;
            let wm_id = self.doc.add_object(Stream::new(dictionary! {}, bytes));
            self.append_content(page_id, wm_id)?;
        }
        Ok(())
    }

    /// Fill AcroForm text fields by name, setting `/V` and flagging the form so
    /// viewers regenerate appearances.
    pub fn fill_form(&mut self, fields: &HashMap<String, String>) -> Result<(), PdfError> {
        // Collect AcroForm field references.
        let field_ids = self.acroform_field_ids();
        for id in field_ids {
            let name = self
                .doc
                .get_dictionary(id)
                .ok()
                .and_then(|d| d.get(b"T").ok())
                .and_then(|o| o.as_str().ok())
                .map(|b| String::from_utf8_lossy(b).into_owned());
            if let Some(name) = name {
                if let Some(value) = fields.get(&name) {
                    if let Ok(dict) = self.doc.get_dictionary_mut(id) {
                        dict.set("V", Object::string_literal(value.as_str()));
                    }
                }
            }
        }
        // NeedAppearances = true so readers regenerate field appearances.
        if let Ok(catalog) = self.doc.catalog() {
            if let Ok(acroform_id) = catalog.get(b"AcroForm").and_then(Object::as_reference) {
                if let Ok(acro) = self.doc.get_dictionary_mut(acroform_id) {
                    acro.set("NeedAppearances", true);
                }
            } else if let Ok(acro) = self.doc.catalog_mut().and_then(|c| c.get_mut(b"AcroForm")) {
                if let Ok(acro) = acro.as_dict_mut() {
                    acro.set("NeedAppearances", true);
                }
            }
        }
        Ok(())
    }

    /// Read the value (`/V`) of an AcroForm text field by name, if present.
    pub fn form_field_value(&self, name: &str) -> Option<String> {
        for id in self.acroform_field_ids() {
            let dict = self.doc.get_dictionary(id).ok()?;
            let field_name = dict
                .get(b"T")
                .ok()
                .and_then(|o| o.as_str().ok())
                .map(|b| String::from_utf8_lossy(b).into_owned());
            if field_name.as_deref() == Some(name) {
                return dict
                    .get(b"V")
                    .ok()
                    .and_then(|o| o.as_str().ok())
                    .map(|b| String::from_utf8_lossy(b).into_owned());
            }
        }
        None
    }

    /// Serialize using object streams (`save_modern`).
    pub fn save(&self, mut out: impl Write) -> Result<(), PdfError> {
        let mut doc = self.doc.clone();
        doc.save_modern(&mut out).map_err(PdfError::from)?;
        Ok(())
    }

    /// Serialize using classic cross-reference tables.
    pub fn save_classic(&self, mut out: impl Write) -> Result<(), PdfError> {
        let mut doc = self.doc.clone();
        let options = SaveOptions::builder()
            .use_object_streams(false)
            .use_xref_streams(false)
            .build();
        doc.save_with_options(&mut out, options)
            .map_err(PdfError::from)?;
        Ok(())
    }

    fn pages_root(&self) -> Result<ObjectId, PdfError> {
        self.doc
            .catalog()
            .map_err(PdfError::from)?
            .get(b"Pages")
            .and_then(Object::as_reference)
            .map_err(PdfError::from)
    }

    fn acroform_field_ids(&self) -> Vec<ObjectId> {
        self.doc
            .catalog()
            .ok()
            .and_then(|c| c.get(b"AcroForm").ok())
            .and_then(|o| match o.as_reference() {
                Ok(id) => self.doc.get_dictionary(id).ok(),
                Err(_) => o.as_dict().ok(),
            })
            .and_then(|acro| acro.get(b"Fields").ok())
            .and_then(|o| o.as_array().ok())
            .map(|arr| arr.iter().filter_map(|o| o.as_reference().ok()).collect())
            .unwrap_or_default()
    }

    /// Add a Helvetica `/PDFKitWM` font to a page's resources if absent.
    fn ensure_watermark_font(&mut self, page_id: ObjectId) -> Result<(), PdfError> {
        let font_id = self.doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = self.page_resources_id(page_id)?;
        let resources = self.doc.get_dictionary_mut(resources_id)?;
        let mut fonts = resources
            .get(b"Font")
            .and_then(Object::as_dict)
            .cloned()
            .unwrap_or_default();
        fonts.set("PDFKitWM", font_id);
        resources.set("Font", fonts);
        Ok(())
    }

    /// Get (or create) a page's own Resources dictionary object id.
    fn page_resources_id(&mut self, page_id: ObjectId) -> Result<ObjectId, PdfError> {
        if let Ok(page) = self.doc.get_dictionary(page_id) {
            if let Ok(id) = page.get(b"Resources").and_then(Object::as_reference) {
                return Ok(id);
            }
            // Inline resources: lift them into their own object.
            if let Ok(dict) = page.get(b"Resources").and_then(Object::as_dict) {
                let dict = dict.clone();
                let id = self.doc.add_object(dict);
                if let Ok(page) = self.doc.get_dictionary_mut(page_id) {
                    page.set("Resources", id);
                }
                return Ok(id);
            }
        }
        // No own /Resources: they're inherited from an ancestor page-tree node.
        // Clone the inherited dict onto the page so the original content's fonts
        // and XObjects survive — otherwise attaching our own (otherwise empty)
        // Resources shadows the inherited ones and the page loses its fonts.
        let dict = self.inherited_resources(page_id).unwrap_or_default();
        let id = self.doc.add_object(dict);
        if let Ok(page) = self.doc.get_dictionary_mut(page_id) {
            page.set("Resources", id);
        }
        Ok(id)
    }

    /// Walk the `/Parent` chain to find `/Resources` inherited by a page, and
    /// return a clone (its font/XObject entries are references, so they stay
    /// valid). Bounded against cyclic parent links.
    fn inherited_resources(&self, page_id: ObjectId) -> Option<Dictionary> {
        let mut current = self
            .doc
            .get_dictionary(page_id)
            .ok()?
            .get(b"Parent")
            .and_then(Object::as_reference)
            .ok();
        for _ in 0..64 {
            let pid = current?;
            let node = self.doc.get_dictionary(pid).ok()?;
            if let Ok(res) = node.get(b"Resources") {
                if let Ok(rid) = res.as_reference() {
                    if let Ok(d) = self.doc.get_dictionary(rid) {
                        return Some(d.clone());
                    }
                } else if let Ok(d) = res.as_dict() {
                    return Some(d.clone());
                }
            }
            current = node.get(b"Parent").and_then(Object::as_reference).ok();
        }
        None
    }

    /// Append a content stream object to a page's Contents.
    fn append_content(&mut self, page_id: ObjectId, content_id: ObjectId) -> Result<(), PdfError> {
        let page = self.doc.get_dictionary_mut(page_id)?;
        let new_contents = match page.get(b"Contents").ok() {
            Some(Object::Array(items)) => {
                let mut items = items.clone();
                items.push(Object::Reference(content_id));
                Object::Array(items)
            }
            Some(Object::Reference(existing)) => Object::Array(vec![
                Object::Reference(*existing),
                Object::Reference(content_id),
            ]),
            _ => Object::Array(vec![Object::Reference(content_id)]),
        };
        page.set("Contents", new_contents);
        Ok(())
    }
}

/// Drop dangling `/Kids` references (to deleted objects) and fix `/Count` on
/// every page-tree node.
fn clean_page_tree(doc: &mut Document) {
    let existing: HashSet<ObjectId> = doc.objects.keys().copied().collect();
    let page_tree_nodes: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, obj)| {
            obj.as_dict()
                .ok()
                .and_then(|d| d.get(b"Type").ok())
                .and_then(|t| t.as_name().ok())
                .map(|t| t == b"Pages")
                .unwrap_or(false)
        })
        .map(|(id, _)| *id)
        .collect();

    for node in page_tree_nodes {
        if let Ok(dict) = doc.get_dictionary_mut(node) {
            if let Ok(kids) = dict.get(b"Kids").and_then(Object::as_array).cloned() {
                let filtered: Vec<Object> = kids
                    .into_iter()
                    .filter(|k| {
                        k.as_reference()
                            .map(|r| existing.contains(&r))
                            .unwrap_or(true)
                    })
                    .collect();
                let count = filtered.len() as i64;
                dict.set("Kids", filtered);
                dict.set("Count", count);
            }
        }
    }
}

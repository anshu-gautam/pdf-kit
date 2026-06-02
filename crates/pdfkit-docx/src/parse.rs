//! Parse a `.docx` (an OPC zip of WordprocessingML XML) into the [`Document`]
//! model. Pure-Rust: `zip` (deflate) to read the package, `roxmltree` to walk
//! the XML, `image` to decode embedded media. We read a pragmatic subset —
//! paragraphs, headings (by style name), runs with bold/italic, lists (bullet /
//! decimal), tables, and inline images — and ignore the rest.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use pdfkit_core::PdfError;
use roxmltree::Node;

use crate::model::{Align, Block, Cell, Document, Image, Para, ParaKind, Table, TextRun};

/// EMU (English Metric Units) per PostScript point. WordprocessingML drawing
/// extents are in EMU; PDF user space is in points.
const EMU_PER_PT: f32 = 12700.0;

/// Parse `.docx` bytes into the document model.
pub fn parse_docx(bytes: &[u8]) -> Result<Document, PdfError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| PdfError::format(DocxError(format!("not a valid .docx (zip): {e}"))))?;

    // Pull the text parts to owned strings and every media blob to a map up
    // front, so the rest of parsing borrows neither the archive nor each other.
    let document_xml = read_string(&mut archive, "word/document.xml").ok_or_else(|| {
        PdfError::format(DocxError(
            "missing word/document.xml — not a Word document".to_string(),
        ))
    })?;
    let rels_xml = read_string(&mut archive, "word/_rels/document.xml.rels");
    let numbering_xml = read_string(&mut archive, "word/numbering.xml");
    let media = read_media(&mut archive);

    let rels = rels_xml.map(|x| parse_rels(&x)).unwrap_or_default();
    let numbering = numbering_xml
        .map(|x| Numbering::parse(&x))
        .unwrap_or_default();

    let tree = roxmltree::Document::parse(&document_xml)
        .map_err(|e| PdfError::format(DocxError(format!("malformed document.xml: {e}"))))?;
    let body = tree
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "body")
        .ok_or_else(|| PdfError::format(DocxError("document.xml has no <w:body>".to_string())))?;

    let mut ctx = Ctx {
        rels: &rels,
        media: &media,
        numbering: &numbering,
        counters: HashMap::new(),
    };
    let mut doc = Document::default();
    for child in body.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "p" => ctx.push_paragraph(child, &mut doc.blocks),
            "tbl" => doc.blocks.push(Block::Table(ctx.parse_table(child))),
            _ => {} // sectPr and friends — ignored
        }
    }
    Ok(doc)
}

/// Mutable state threaded through the body walk.
struct Ctx<'a> {
    rels: &'a HashMap<String, String>,
    media: &'a HashMap<String, Vec<u8>>,
    numbering: &'a Numbering,
    /// Running counters for ordered lists, keyed (numId, ilvl).
    counters: HashMap<(String, u8), u32>,
}

impl Ctx<'_> {
    /// Parse a `<w:p>` and append the resulting paragraph and/or image blocks.
    fn push_paragraph(&mut self, p: Node, out: &mut Vec<Block>) {
        let ppr = child(p, "pPr");
        let align = ppr
            .and_then(|n| child(n, "jc"))
            .and_then(alignment)
            .unwrap_or_default();

        let style = ppr
            .and_then(|n| child(n, "pStyle"))
            .and_then(|n| attr(n, "val"))
            .map(str::to_string);

        // Heading / title detection from the style id (Word uses "Heading1"..
        // "Heading9", "Title").
        let heading_kind = style.as_deref().and_then(heading_kind);

        // List detection: a numbering reference, unless it's a heading.
        let num_pr = ppr.and_then(|n| child(n, "numPr"));
        let kind = match (heading_kind, num_pr) {
            (Some(k), _) => k,
            (None, Some(np)) => self.list_kind(np),
            (None, None) => ParaKind::Body,
        };

        let mut runs: Vec<TextRun> = Vec::new();
        let mut images: Vec<Image> = Vec::new();
        collect_runs(p, self.rels, self.media, &mut runs, &mut images);

        let para = Para { kind, align, runs };
        // Emit the text paragraph unless it is an empty list/heading shell.
        let has_text = !para.is_empty();
        let keep_para = has_text || matches!(para.kind, ParaKind::Body);
        if keep_para && (has_text || images.is_empty()) {
            out.push(Block::Para(para));
        }
        for img in images {
            out.push(Block::Image(img));
        }
    }

    /// Resolve a `<w:numPr>` to a list-item kind with a rendered marker.
    fn list_kind(&mut self, num_pr: Node) -> ParaKind {
        let ilvl = child(num_pr, "ilvl")
            .and_then(|n| attr(n, "val"))
            .and_then(|v| v.parse::<u8>().ok())
            .unwrap_or(0);
        let num_id = child(num_pr, "numId")
            .and_then(|n| attr(n, "val"))
            .unwrap_or("")
            .to_string();

        // Word's default: advancing a level restarts the numbering of all deeper
        // levels for the same list, so `1) a) b) 2) a) b)` doesn't run the inner
        // counter 1,2,3,4 across both parents. Drop deeper counters on each item
        // (ordered or bullet) so the next descent restarts at 1.
        self.counters
            .retain(|(nid, lvl), _| nid != &num_id || *lvl <= ilvl);

        let ordered = self.numbering.is_ordered(&num_id, ilvl);
        let marker = if ordered {
            let n = self.counters.entry((num_id, ilvl)).or_insert(0);
            // Saturating: the marker is cosmetic and input is untrusted, so a
            // pathological count must never overflow-panic in a debug build.
            *n = n.saturating_add(1);
            format!("{n}. ")
        } else {
            "•  ".to_string()
        };
        ParaKind::ListItem {
            marker,
            level: ilvl,
        }
    }

    /// Parse a `<w:tbl>` into a [`Table`] (cells hold text paragraphs only).
    fn parse_table(&mut self, tbl: Node) -> Table {
        let mut rows = Vec::new();
        for tr in tbl
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "tr")
        {
            let mut cells = Vec::new();
            for tc in tr
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "tc")
            {
                let mut cell = Cell::default();
                for p in tc
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "p")
                {
                    let align = child(p, "pPr")
                        .and_then(|n| child(n, "jc"))
                        .and_then(alignment)
                        .unwrap_or_default();
                    let mut runs = Vec::new();
                    let mut imgs = Vec::new();
                    collect_runs(p, self.rels, self.media, &mut runs, &mut imgs);
                    cell.paras.push(Para {
                        kind: ParaKind::Body,
                        align,
                        runs,
                    });
                }
                cells.push(cell);
            }
            if !cells.is_empty() {
                rows.push(cells);
            }
        }
        Table { rows }
    }
}

/// Collect the styled text runs and inline images under a `<w:p>` (descending
/// into hyperlinks), appending to `runs` / `images`.
fn collect_runs(
    p: Node,
    rels: &HashMap<String, String>,
    media: &HashMap<String, Vec<u8>>,
    runs: &mut Vec<TextRun>,
    images: &mut Vec<Image>,
) {
    for node in p.children().filter(Node::is_element) {
        match node.tag_name().name() {
            "r" => parse_run(node, rels, media, runs, images),
            // Hyperlinks wrap runs; flatten them in (link target is dropped).
            "hyperlink" => {
                for r in node
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "r")
                {
                    parse_run(r, rels, media, runs, images);
                }
            }
            _ => {}
        }
    }
}

/// Parse a single `<w:r>` run.
fn parse_run(
    r: Node,
    rels: &HashMap<String, String>,
    media: &HashMap<String, Vec<u8>>,
    runs: &mut Vec<TextRun>,
    images: &mut Vec<Image>,
) {
    let rpr = child(r, "rPr");
    let bold = rpr.map(|n| toggle(n, "b")).unwrap_or(false);
    let italic = rpr.map(|n| toggle(n, "i")).unwrap_or(false);

    let mut text = String::new();
    for node in r.children().filter(Node::is_element) {
        match node.tag_name().name() {
            "t" => text.push_str(node.text().unwrap_or("")),
            "tab" => text.push('\t'),
            "br" | "cr" => text.push('\n'),
            "drawing" => {
                if let Some(img) = parse_drawing(node, rels, media) {
                    images.push(img);
                }
            }
            _ => {}
        }
    }
    if !text.is_empty() {
        runs.push(TextRun { text, bold, italic });
    }
}

/// Extract an inline image from a `<w:drawing>`: resolve the `a:blip` embed
/// relationship to a media blob, decode it, and re-encode as PNG for placement.
fn parse_drawing(
    drawing: Node,
    rels: &HashMap<String, String>,
    media: &HashMap<String, Vec<u8>>,
) -> Option<Image> {
    let extent = drawing
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "extent");
    let pt_w = extent
        .and_then(|n| attr(n, "cx"))
        .and_then(|v| v.parse::<f32>().ok())
        .map(|emu| emu / EMU_PER_PT);
    let pt_h = extent
        .and_then(|n| attr(n, "cy"))
        .and_then(|v| v.parse::<f32>().ok())
        .map(|emu| emu / EMU_PER_PT);

    let blip = drawing
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "blip")?;
    let rid = attr(blip, "embed").or_else(|| attr(blip, "link"))?;
    let target = rels.get(rid)?;
    let path = normalize_media_path(target);
    let raw = media.get(&path)?;

    // Decode whatever format the blob is (png/jpeg/…) and re-encode to PNG so it
    // can go through the edit path's `place_image`.
    let decoded = image::load_from_memory(raw).ok()?;
    let rgba = decoded.to_rgba8();
    let (px_w, px_h) = (rgba.width(), rgba.height());
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;

    Some(Image {
        png,
        px_w,
        px_h,
        pt_w,
        pt_h,
    })
}

// --- relationships & numbering --------------------------------------------

/// Parse `word/_rels/document.xml.rels` into `rId -> Target`.
fn parse_rels(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(doc) = roxmltree::Document::parse(xml) {
        for n in doc
            .descendants()
            .filter(|n| n.tag_name().name() == "Relationship")
        {
            if let (Some(id), Some(target)) = (n.attribute("Id"), n.attribute("Target")) {
                map.insert(id.to_string(), target.to_string());
            }
        }
    }
    map
}

/// A relationship `Target` is usually relative to the `word/` part
/// (e.g. `media/image1.png`); normalize it to a full zip path.
fn normalize_media_path(target: &str) -> String {
    let t = target.trim_start_matches('/');
    if t.starts_with("word/") {
        t.to_string()
    } else {
        format!("word/{t}")
    }
}

/// Minimal numbering map: which (numId, level) render as ordered (decimal-ish)
/// vs. bullet lists.
#[derive(Default)]
struct Numbering {
    num_to_abstract: HashMap<String, String>,
    /// (abstractNumId, ilvl) -> is-ordered.
    ordered: HashMap<(String, u8), bool>,
}

impl Numbering {
    fn parse(xml: &str) -> Numbering {
        let mut n = Numbering::default();
        let Ok(doc) = roxmltree::Document::parse(xml) else {
            return n;
        };
        for abs in doc
            .descendants()
            .filter(|x| x.tag_name().name() == "abstractNum")
        {
            let Some(abs_id) = attr(abs, "abstractNumId") else {
                continue;
            };
            for lvl in abs
                .children()
                .filter(|x| x.is_element() && x.tag_name().name() == "lvl")
            {
                let ilvl = attr(lvl, "ilvl")
                    .and_then(|v| v.parse::<u8>().ok())
                    .unwrap_or(0);
                let fmt = child(lvl, "numFmt")
                    .and_then(|f| attr(f, "val"))
                    .unwrap_or("bullet");
                n.ordered
                    .insert((abs_id.to_string(), ilvl), fmt != "bullet" && fmt != "none");
            }
        }
        for num in doc.descendants().filter(|x| x.tag_name().name() == "num") {
            let Some(num_id) = attr(num, "numId") else {
                continue;
            };
            if let Some(abs_id) = child(num, "abstractNumId").and_then(|a| attr(a, "val")) {
                n.num_to_abstract
                    .insert(num_id.to_string(), abs_id.to_string());
            }
        }
        n
    }

    fn is_ordered(&self, num_id: &str, ilvl: u8) -> bool {
        self.num_to_abstract
            .get(num_id)
            .and_then(|abs| self.ordered.get(&(abs.clone(), ilvl)))
            .copied()
            .unwrap_or(false)
    }
}

// --- small XML helpers ------------------------------------------------------

/// First child element with the given local name.
fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
}

/// An attribute value matched by local name (namespace-agnostic).
fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}

/// A boolean WordprocessingML toggle (`<w:b/>`, `<w:i/>`): present means on
/// unless an explicit `w:val` says otherwise.
fn toggle(rpr: Node, name: &str) -> bool {
    match child(rpr, name) {
        None => false,
        Some(el) => !matches!(attr(el, "val"), Some("false") | Some("0") | Some("off")),
    }
}

/// Map a `<w:jc>` justification to an [`Align`].
fn alignment(jc: Node) -> Option<Align> {
    Some(match attr(jc, "val")? {
        "center" => Align::Center,
        "right" | "end" => Align::Right,
        "both" | "distribute" => Align::Justify,
        _ => Align::Left,
    })
}

/// Map a paragraph style id to a heading/title kind, if it is one.
fn heading_kind(style: &str) -> Option<ParaKind> {
    let s = style.to_ascii_lowercase().replace(['-', '_', ' '], "");
    if s == "title" {
        return Some(ParaKind::Title);
    }
    if let Some(rest) = s.strip_prefix("heading") {
        if let Ok(level) = rest.parse::<u8>() {
            return Some(ParaKind::Heading(level.clamp(1, 6)));
        }
    }
    None
}

// --- zip helpers ------------------------------------------------------------

fn read_string<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<String> {
    let mut f = archive.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

/// Read every `word/media/*` entry into a path -> bytes map.
fn read_media<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> HashMap<String, Vec<u8>> {
    let mut out = HashMap::new();
    for i in 0..archive.len() {
        let Ok(mut f) = archive.by_index(i) else {
            continue;
        };
        let name = f.name().to_string();
        if name.starts_with("word/media/") {
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                out.insert(name, buf);
            }
        }
    }
    out
}

/// A docx-specific parse error, surfaced as the source of a [`PdfError::Format`]
/// so the API maps it to a 4xx rather than a server fault.
#[derive(Debug)]
struct DocxError(String);

impl std::fmt::Display for DocxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DocxError {}

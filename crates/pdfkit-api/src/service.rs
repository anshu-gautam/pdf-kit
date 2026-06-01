//! Service logic: pure functions over bytes + parsed options, returning DTOs or
//! raw output bytes. Kept free of axum so it can be unit-tested directly against
//! fixtures. The thin HTTP layer lives in `handlers`.

use std::collections::HashMap;

use pdfkit_core::{Document, Engine, OpenOptions, PdfError};
use pdfkit_edit::{PdfEditor, WatermarkOptions};

use crate::dto::*;

/// Open a document for the read path (extract uses its own entry point).
fn open_doc(bytes: Vec<u8>, password: Option<String>) -> Result<Document, PdfError> {
    Engine::new()?.open(bytes, OpenOptions { password })
}

/// Serialize an editor's current document to PDF bytes.
fn save_editor(editor: &PdfEditor) -> Result<Vec<u8>, PdfError> {
    let mut out = Vec::new();
    editor.save(&mut out)?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Read path
// ---------------------------------------------------------------------------

pub fn run_extract(bytes: Vec<u8>, req: &ExtractRequest) -> Result<ExtractResponse, PdfError> {
    Ok(pdfkit_core::extract(bytes, req.to_options())?.into())
}

pub fn run_metadata(
    bytes: Vec<u8>,
    password: Option<String>,
) -> Result<MetadataResponse, PdfError> {
    let doc = open_doc(bytes, password)?;
    let m = doc.metadata().clone();
    let outline = doc.outline().iter().map(OutlineNode::from).collect();

    let mut links = Vec::new();
    for p in 1..=doc.page_count() {
        let page_links = doc.page(p)?.links();
        if !page_links.is_empty() {
            links.push(PageLinks {
                page: p,
                links: page_links.iter().map(LinkDto::from).collect(),
            });
        }
    }

    Ok(MetadataResponse {
        page_count: m.page_count,
        title: m.title,
        author: m.author,
        subject: m.subject,
        keywords: m.keywords,
        creator: m.creator,
        producer: m.producer,
        creation_date: m.creation_date,
        mod_date: m.mod_date,
        pdf_version: m.pdf_version,
        encrypted: m.encrypted,
        outline,
        links,
    })
}

/// The serialized chunk output, ready for an HTTP response.
pub enum ChunkOutput {
    Json(serde_json::Value),
    Markdown(String),
}

pub fn run_chunks(bytes: Vec<u8>, req: &ChunkRequest) -> Result<ChunkOutput, PdfError> {
    let doc = open_doc(bytes, req.password.clone())?;
    let chunks = pdfkit_chunk::chunk_document(&doc, &req.to_options())?;
    match req.format {
        ChunkFormat::Json => {
            let chunks_value: serde_json::Value =
                serde_json::from_str(&pdfkit_chunk::to_json(&chunks)?)
                    .map_err(|e| PdfError::Backend(format!("chunk json: {e}")))?;
            Ok(ChunkOutput::Json(serde_json::json!({
                "chunks": chunks_value,
                "document_text": pdfkit_chunk::document_text(&chunks),
            })))
        }
        ChunkFormat::Markdown => Ok(ChunkOutput::Markdown(pdfkit_chunk::to_markdown(&chunks))),
    }
}

pub fn run_figures(bytes: Vec<u8>, password: Option<String>) -> Result<FiguresResponse, PdfError> {
    let doc = open_doc(bytes, password)?;
    let mut pages = Vec::new();
    for p in 1..=doc.page_count() {
        let regions = doc.page(p)?.image_regions();
        if !regions.is_empty() {
            pages.push(PageFigures {
                page: p,
                figures: regions
                    .into_iter()
                    .map(|r| FigureDto {
                        bbox: r.bbox,
                        caption: r.caption,
                    })
                    .collect(),
            });
        }
    }
    Ok(FiguresResponse { pages })
}

/// Render a one-based page to PNG. Only available on a `render-pdfium` build;
/// the native backend can't rasterize text (PRD §13.2).
#[cfg(feature = "render-pdfium")]
pub fn run_render(
    bytes: Vec<u8>,
    params: &RenderParams,
    password: Option<String>,
) -> Result<Vec<u8>, PdfError> {
    let renderer = pdfkit_render::PdfiumRenderer::new()?;
    let bitmap = renderer.render_page(
        &bytes,
        params.page,
        password.as_deref(),
        &params.to_render_options(),
    )?;
    pdfkit_render::encode_png(&bitmap, true)
}

// ---------------------------------------------------------------------------
// Write path (PdfEditor)
// ---------------------------------------------------------------------------

pub fn run_merge(files: Vec<Vec<u8>>) -> Result<Vec<u8>, PdfError> {
    let mut it = files.into_iter();
    let first = it
        .next()
        .ok_or_else(|| PdfError::Backend("merge needs at least one file".into()))?;
    let mut editor = PdfEditor::open(first)?;
    for f in it {
        let other = PdfEditor::open(f)?;
        editor.merge(&other)?;
    }
    save_editor(&editor)
}

pub fn run_split(bytes: Vec<u8>, ranges: &[(usize, usize)]) -> Result<Vec<Vec<u8>>, PdfError> {
    PdfEditor::open(bytes)?.split(ranges)
}

pub fn run_rotate(bytes: Vec<u8>, rotations: &[(usize, i32)]) -> Result<Vec<u8>, PdfError> {
    let mut editor = PdfEditor::open(bytes)?;
    for &(page, degrees) in rotations {
        editor.rotate_page(page, degrees)?;
    }
    save_editor(&editor)
}

pub fn run_watermark(bytes: Vec<u8>, req: WatermarkRequest) -> Result<Vec<u8>, PdfError> {
    let mut editor = PdfEditor::open(bytes)?;
    let d = WatermarkOptions::default();
    let opts = WatermarkOptions {
        font_size: req.font_size.unwrap_or(d.font_size),
        gray: req.gray.unwrap_or(d.gray),
        rotation_degrees: req.rotation_degrees.unwrap_or(d.rotation_degrees),
    };
    editor.watermark(&req.text, opts)?;
    save_editor(&editor)
}

pub fn run_fill(bytes: Vec<u8>, fields: HashMap<String, String>) -> Result<Vec<u8>, PdfError> {
    let mut editor = PdfEditor::open(bytes)?;
    editor.fill_form(&fields)?;
    save_editor(&editor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
    }

    fn page_count(pdf: &[u8]) -> usize {
        PdfEditor::open(pdf.to_vec())
            .expect("reopen output")
            .page_count()
    }

    #[test]
    fn extract_text_from_born_digital() {
        let req = ExtractRequest {
            mode: ExtractMode::Text,
            ..ExtractRequest::default()
        };
        let r = run_extract(fixture("born-digital.pdf"), &req).expect("extract");
        assert!(!r.text.trim().is_empty(), "expected recovered text");
        assert!(r.page_images.is_empty(), "text mode emits no images");
    }

    #[test]
    fn metadata_has_outline_and_links() {
        let r = run_metadata(fixture("outline-and-links.pdf"), None).expect("metadata");
        assert!(r.page_count >= 1);
        assert!(!r.outline.is_empty(), "expected bookmarks");
        assert!(!r.links.is_empty(), "expected per-page links");
    }

    #[test]
    fn chunks_json_then_markdown() {
        let mut req = ChunkRequest::default();
        match run_chunks(fixture("multi-heading.pdf"), &req).expect("chunks json") {
            ChunkOutput::Json(v) => {
                assert!(v["chunks"].is_array());
                assert!(v["document_text"].is_string());
            }
            ChunkOutput::Markdown(_) => panic!("expected json"),
        }
        req.format = ChunkFormat::Markdown;
        match run_chunks(fixture("multi-heading.pdf"), &req).expect("chunks md") {
            ChunkOutput::Markdown(s) => assert!(!s.trim().is_empty()),
            ChunkOutput::Json(_) => panic!("expected markdown"),
        }
    }

    #[test]
    fn figures_are_detected() {
        let r = run_figures(fixture("figure-with-caption.pdf"), None).expect("figures");
        assert!(!r.pages.is_empty(), "expected at least one figure page");
    }

    #[test]
    fn merge_doubles_page_count() {
        let one = page_count(&fixture("born-digital.pdf"));
        let merged = run_merge(vec![
            fixture("born-digital.pdf"),
            fixture("born-digital.pdf"),
        ])
        .expect("merge");
        assert_eq!(page_count(&merged), one * 2);
    }

    #[test]
    fn split_returns_one_pdf_per_range() {
        let parts = run_split(fixture("multi-heading.pdf"), &[(1, 1)]).expect("split");
        assert_eq!(parts.len(), 1);
        assert_eq!(page_count(&parts[0]), 1);
    }

    #[test]
    fn rotate_reopens() {
        let out = run_rotate(fixture("born-digital.pdf"), &[(1, 90)]).expect("rotate");
        assert!(page_count(&out) >= 1);
    }

    #[test]
    fn watermark_reopens() {
        let req = WatermarkRequest {
            text: "DRAFT".to_string(),
            font_size: None,
            gray: None,
            rotation_degrees: None,
        };
        let out = run_watermark(fixture("born-digital.pdf"), req).expect("watermark");
        assert!(page_count(&out) >= 1);
    }

    #[test]
    fn fill_form_reopens() {
        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "Ada".to_string());
        let out = run_fill(fixture("forms.pdf"), fields).expect("fill");
        assert!(page_count(&out) >= 1);
    }
}

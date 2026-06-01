//! Request/response DTOs (PRD §13.3).
//!
//! `pdfkit-core` is serde-free and its result types do not derive `Serialize`,
//! so the API owns these DTOs and converts to/from the library types by hand.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Error envelope returned for every non-2xx response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApiError {
    #[schema(value_type = String)]
    pub code: &'static str,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Render parameters (shared by /v1/render and the extract image options).
// ---------------------------------------------------------------------------

/// Page background. Mirrors `pdfkit_core::Background` (White | Transparent) —
/// not an arbitrary color.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Background {
    #[default]
    White,
    Transparent,
}

/// The full real render surface. Sizing precedence (in `pdfkit-core`): explicit
/// width/height → dpi → scale → 150 DPI default.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RenderParams {
    /// One-based page number (used by /v1/render; ignored by extract).
    pub page: usize,
    pub dpi: Option<f32>,
    pub scale: Option<f32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub background: Background,
    // NOTE: the pixel/dimension safety budget is intentionally NOT client-settable
    // — a caller must not be able to raise the core render budget (DoS guard).
}

impl Default for RenderParams {
    fn default() -> Self {
        RenderParams {
            page: 1,
            dpi: None,
            scale: None,
            width: None,
            height: None,
            background: Background::White,
        }
    }
}

impl RenderParams {
    pub fn to_render_options(&self) -> pdfkit_core::RenderOptions {
        let d = pdfkit_core::RenderOptions::default();
        pdfkit_core::RenderOptions {
            dpi: self.dpi,
            scale: self.scale,
            width: self.width,
            height: self.height,
            background: match self.background {
                Background::White => pdfkit_core::Background::White,
                Background::Transparent => pdfkit_core::Background::Transparent,
            },
            forms: d.forms,
            // Server-controlled budget — clients cannot widen it.
            max_pixels: d.max_pixels,
            max_dimension: d.max_dimension,
        }
    }
}

// ---------------------------------------------------------------------------
// /v1/extract
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtractMode {
    #[default]
    Auto,
    Text,
    Images,
    Both,
}

impl From<ExtractMode> for pdfkit_core::Mode {
    fn from(m: ExtractMode) -> Self {
        match m {
            ExtractMode::Auto => pdfkit_core::Mode::Auto,
            ExtractMode::Text => pdfkit_core::Mode::Text,
            ExtractMode::Images => pdfkit_core::Mode::Images,
            ExtractMode::Both => pdfkit_core::Mode::Both,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ExtractRequest {
    pub mode: ExtractMode,
    pub password: Option<String>,
    /// One-based page numbers, in order.
    pub pages: Option<Vec<usize>>,
    pub max_pages: usize,
    pub min_text_chars: usize,
    pub max_text_chars: usize,
    /// Accepted for forward-compat; only effective on an `ocr` build (PRD §16).
    pub ocr: bool,
    /// How to render page images when `mode` includes images.
    pub render: RenderParams,
}

impl Default for ExtractRequest {
    fn default() -> Self {
        let o = pdfkit_core::ExtractOptions::default();
        ExtractRequest {
            mode: ExtractMode::Auto,
            password: None,
            pages: None,
            max_pages: o.max_pages,
            min_text_chars: o.min_text_chars,
            max_text_chars: o.max_text_chars,
            ocr: false,
            render: RenderParams::default(),
        }
    }
}

impl ExtractRequest {
    pub fn to_options(&self) -> pdfkit_core::ExtractOptions {
        pdfkit_core::ExtractOptions {
            mode: self.mode.into(),
            password: self.password.clone(),
            pages: self.pages.clone(),
            max_pages: self.max_pages,
            min_text_chars: self.min_text_chars,
            max_text_chars: self.max_text_chars,
            ocr: self.ocr,
            image: self.render.to_render_options(),
        }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ExtractResponse {
    pub text: String,
    /// Rendered page images (PNG, base64) — NOT extracted embedded images.
    pub page_images: Vec<PageImage>,
    pub pages_processed: Vec<usize>,
    pub truncated: TruncatedDto,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PageImage {
    pub page: usize,
    pub width: u32,
    pub height: u32,
    pub png_base64: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TruncatedDto {
    pub text: bool,
    pub images: bool,
}

impl From<pdfkit_core::ExtractResult> for ExtractResponse {
    fn from(r: pdfkit_core::ExtractResult) -> Self {
        use base64::Engine as _;
        let encoder = base64::engine::general_purpose::STANDARD;
        ExtractResponse {
            text: r.text,
            page_images: r
                .images
                .into_iter()
                .map(|i| PageImage {
                    page: i.page,
                    width: i.width,
                    height: i.height,
                    png_base64: encoder.encode(&i.png),
                })
                .collect(),
            pages_processed: r.pages_processed,
            truncated: TruncatedDto {
                text: r.truncated.text,
                images: r.truncated.images,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// /v1/metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MetadataRequest {
    pub password: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct MetadataResponse {
    pub page_count: usize,
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub creation_date: Option<String>,
    pub mod_date: Option<String>,
    pub pdf_version: String,
    pub encrypted: bool,
    pub outline: Vec<OutlineNode>,
    pub links: Vec<PageLinks>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OutlineNode {
    pub title: String,
    pub page: Option<usize>,
    // Recursive (bookmark tree): tell utoipa to $ref instead of inlining, which
    // would overflow the stack while building the OpenAPI document.
    #[schema(no_recursion)]
    pub children: Vec<OutlineNode>,
}

impl From<&pdfkit_core::OutlineItem> for OutlineNode {
    fn from(o: &pdfkit_core::OutlineItem) -> Self {
        OutlineNode {
            title: o.title.clone(),
            page: o.page,
            children: o.children.iter().map(OutlineNode::from).collect(),
        }
    }
}

/// Links collected per page (links are a per-page concept in the library).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PageLinks {
    pub page: usize,
    pub links: Vec<LinkDto>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LinkDto {
    pub rect: [f32; 4],
    pub target: LinkTargetDto,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LinkTargetDto {
    Uri { uri: String },
    Page { page: usize },
}

impl From<&pdfkit_core::Link> for LinkDto {
    fn from(l: &pdfkit_core::Link) -> Self {
        LinkDto {
            rect: l.rect,
            target: match &l.target {
                pdfkit_core::LinkTarget::Uri(u) => LinkTargetDto::Uri { uri: u.clone() },
                pdfkit_core::LinkTarget::Page(p) => LinkTargetDto::Page { page: *p },
            },
        }
    }
}

// ---------------------------------------------------------------------------
// /v1/chunks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChunkFormat {
    #[default]
    Json,
    Markdown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ChunkRequest {
    pub format: ChunkFormat,
    pub password: Option<String>,
    pub target_tokens: usize,
    pub overlap_tokens: usize,
    pub respect_boundaries: bool,
    pub contextual_prefix: bool,
}

impl Default for ChunkRequest {
    fn default() -> Self {
        let o = pdfkit_chunk::ChunkOptions::default();
        ChunkRequest {
            format: ChunkFormat::Json,
            password: None,
            target_tokens: o.target_tokens,
            overlap_tokens: o.overlap_tokens,
            respect_boundaries: o.respect_boundaries,
            contextual_prefix: o.contextual_prefix,
        }
    }
}

impl ChunkRequest {
    pub fn to_options(&self) -> pdfkit_chunk::ChunkOptions {
        pdfkit_chunk::ChunkOptions {
            target_tokens: self.target_tokens,
            overlap_tokens: self.overlap_tokens,
            respect_boundaries: self.respect_boundaries,
            contextual_prefix: self.contextual_prefix,
        }
    }
}

// ---------------------------------------------------------------------------
// /v1/figures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FiguresRequest {
    pub password: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FiguresResponse {
    pub pages: Vec<PageFigures>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PageFigures {
    pub page: usize,
    pub figures: Vec<FigureDto>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FigureDto {
    pub bbox: [f32; 4],
    pub caption: Option<String>,
}

// ---------------------------------------------------------------------------
// /v1/render
// ---------------------------------------------------------------------------

// Only used by the render handler, which exists on a `render-pdfium` build.
#[cfg(feature = "render-pdfium")]
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RenderRequest {
    #[serde(flatten)]
    pub params: RenderParams,
    pub password: Option<String>,
}

// ---------------------------------------------------------------------------
// /v1/edit/* (PdfEditor cannot open encrypted inputs, so no password here)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct SplitRequest {
    /// One-based inclusive `[start, end]` page ranges; one output PDF per range.
    pub ranges: Vec<[usize; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RotateRequest {
    pub rotations: Vec<RotateItem>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct RotateItem {
    /// One-based page.
    pub page: usize,
    /// Multiple of 90 degrees.
    pub degrees: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WatermarkRequest {
    pub text: String,
    pub font_size: Option<f32>,
    pub gray: Option<f32>,
    pub rotation_degrees: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FillRequest {
    pub fields: HashMap<String, String>,
}

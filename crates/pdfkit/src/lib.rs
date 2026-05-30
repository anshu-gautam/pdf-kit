//! `pdfkit` — umbrella crate re-exporting the toolkit (PRD §7).
//!
//! Pulls together the `pdfkit-*` crates behind feature flags so consumers can
//! depend on one crate:
//!
//! ```toml
//! pdfkit = "0.1"                                   # default: render-native, edit, chunk, adapters
//! pdfkit = { version = "0.1", features = ["render-pdfium", "ocr-ocrs"] }
//! ```
//!
//! The deterministic, offline read path (`extract`, `Document`, classification,
//! native render) is always available; `edit`, `chunk`, `adapters`, the PDFIUM
//! backend, OCR, and the LLM adapter are opt-in features.

// Core read path + shared types (always available).
pub use pdfkit_core::{
    extract, ocr_page, Background, Bitmap, Document, Engine, ExtractOptions, ExtractResult,
    Metadata, Mode, OcrProvider, OcrResult, OcrWord, OpenOptions, Page, PageKind, PageSignals,
    PdfError, PdfImage, PdfInput, RenderOptions, Renderer, TextOptions, TextRun, Truncated,
};

#[cfg(feature = "render-native")]
pub use pdfkit_core::{encode_png, extract_with_ocr, NativeRenderer};

/// High-fidelity PDFIUM renderer (`render-pdfium`).
#[cfg(feature = "render-pdfium")]
pub use pdfkit_render::PdfiumRenderer;

/// Concrete OCR providers (`ocr-ocrs` / `ocr-tesseract`).
#[cfg(feature = "ocr-ocrs")]
pub use pdfkit_ocr::OcrsProvider;
#[cfg(feature = "ocr-tesseract")]
pub use pdfkit_ocr::TesseractProvider;

/// Structured / RAG chunking (`chunk`).
#[cfg(feature = "chunk")]
pub use pdfkit_chunk::{chunk_document, Chunk, ChunkOptions, ElementKind};

/// The write path (`edit`).
#[cfg(feature = "edit")]
pub use pdfkit_edit::{
    FontFamily, FontSpec, PageRef, PageSize, PdfBuilder, PdfEditor, WatermarkOptions,
};

#[cfg(all(feature = "adapters", feature = "llm-adapter"))]
pub use pdfkit_adapters::{title_chunks, LlmClient};
/// Model-ready adapters (`adapters`).
#[cfg(feature = "adapters")]
pub use pdfkit_adapters::{to_data_urls, to_message_content, ContentBlock};

/// Provenance of the PDFIUM binary used by the `render-pdfium` backend. The
/// library is downloaded by `scripts/fetch-pdfium.sh`, never vendored.
#[cfg(feature = "render-pdfium")]
pub mod pdfium_provenance {
    /// Upstream source of the prebuilt PDFIUM binary.
    pub const SOURCE: &str = "https://github.com/bblanchon/pdfium-binaries";
    /// Release tag this backend was verified against.
    pub const RELEASE_TAG: &str = "chromium/7857";
}

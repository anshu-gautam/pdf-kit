//! `pdfkit-core` — the foundation crate.
//!
//! Owns the document model, text extraction, page classification, and the
//! [`extract`] entry point. Everything in the workspace depends on this crate.
//!
//! Implemented incrementally per the milestones in `Prd.md`:
//! - M1: `Engine`, `Document`, `Page`, `Metadata`, `PdfError`, text extraction.
//! - M2: page classification.
//! - M4: the `extract` Auto fallback.

mod classify;
mod document;
mod error;
mod extract;
mod geometry;
pub mod layout;
mod ocr;
mod render;
mod textrun;
mod types;

pub use classify::{PageKind, PageSignals};
pub use document::{Document, Engine, Metadata, Page};
pub use error::PdfError;
pub use extract::{extract, ExtractOptions, ExtractResult, Mode, PdfImage, Truncated};
pub use layout::{group_runs_into_lines, Cell, Line, COLUMN_GAP};
pub use ocr::{ocr_page, OcrProvider, OcrResult, OcrWord};
pub use render::{Background, Bitmap, RenderOptions, Renderer};
pub use textrun::TextRun;
pub use types::{OpenOptions, PdfInput, TextOptions};

#[cfg(feature = "render-native")]
pub use extract::extract_with_ocr;
#[cfg(feature = "render-native")]
pub use render::{encode_png, NativeRenderer};

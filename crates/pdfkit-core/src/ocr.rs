//! OCR abstraction (PRD §4.3).
//!
//! The [`OcrProvider`] trait, result types, and [`ocr_page`] are dependency-free
//! and always compiled. Concrete providers (local ONNX via `ocrs`, or system
//! Tesseract) live in `pdfkit-ocr` behind feature flags and are passed in by the
//! caller — so the in-core `extract_with_ocr` can use OCR without a dependency
//! cycle.

use crate::document::Page;
use crate::error::PdfError;
use crate::render::{Bitmap, RenderOptions, Renderer};

/// A single recognized word with its location and confidence.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrWord {
    /// The recognized text.
    pub text: String,
    /// Bounding box `[x0, y0, x1, y1]` in pixels of the rendered bitmap.
    pub bbox: [f32; 4],
    /// Per-word confidence in `0.0..=1.0`.
    pub confidence: f32,
}

/// The result of recognizing a page image.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OcrResult {
    /// The full recovered text.
    pub text: String,
    /// Overall confidence in `0.0..=1.0`.
    pub confidence: f32,
    /// Per-word detail (may be empty if a provider doesn't supply it).
    pub words: Vec<OcrWord>,
}

/// Recognize text from a rendered bitmap.
pub trait OcrProvider {
    /// Recognize text in `bmp`.
    fn recognize(&self, bmp: &Bitmap) -> Result<OcrResult, PdfError>;
}

/// Rasterize a page with `renderer`, then recognize it with `provider`.
///
/// The page is rendered at an OCR-friendly resolution (300 DPI) with a raised
/// pixel budget, since recognition benefits from detail.
pub fn ocr_page<R, P>(page: &Page, renderer: &R, provider: &P) -> Result<OcrResult, PdfError>
where
    R: Renderer,
    P: OcrProvider,
{
    let opts = RenderOptions {
        dpi: Some(300.0),
        max_pixels: 30_000_000,
        ..RenderOptions::default()
    };
    let bitmap = renderer.render(page, &opts)?;
    provider.recognize(&bitmap)
}

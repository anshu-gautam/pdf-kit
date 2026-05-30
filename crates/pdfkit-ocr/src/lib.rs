//! `pdfkit-ocr` — rasterize + OCR scanned pages.
//!
//! The OCR abstraction ([`OcrProvider`], [`OcrResult`], [`OcrWord`],
//! [`ocr_page`]) is defined in `pdfkit-core` so the in-core `extract_with_ocr`
//! can use any provider without a dependency cycle. This crate re-exports that
//! abstraction and adds concrete providers behind feature flags:
//!
//! - `ocr-ocrs`: [`OcrsProvider`], local ONNX recognition via `ocrs` + `rten`.
//!   Models are fetched by `scripts/fetch-ocr-models.sh` into a cache dir.
//! - `ocr-tesseract`: [`TesseractProvider`], using the system Tesseract library.
//!
//! Both backends discover their models/dependencies at construction time and
//! report a clear [`PdfError`] when they are missing.

pub use pdfkit_core::{ocr_page, Bitmap, OcrProvider, OcrResult, OcrWord, PdfError};

// Convenience re-export so callers can rasterize for OCR without depending on
// pdfkit-core directly.
pub use pdfkit_core::{NativeRenderer, RenderOptions, Renderer};

#[cfg(feature = "ocr-ocrs")]
pub use ocrs_backend::OcrsProvider;

#[cfg(feature = "ocr-tesseract")]
pub use tesseract_backend::TesseractProvider;

/// Locate the OCR model cache directory: `$PDFKIT_OCR_MODELS` if set, else
/// `$XDG_CACHE_HOME/pdfkit/models`, else `~/.cache/pdfkit/models`.
#[cfg(any(feature = "ocr-ocrs", feature = "ocr-tesseract"))]
fn models_dir() -> std::path::PathBuf {
    use std::path::PathBuf;
    if let Ok(dir) = std::env::var("PDFKIT_OCR_MODELS") {
        return PathBuf::from(dir);
    }
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|_| PathBuf::from(".cache"));
    base.join("pdfkit").join("models")
}

#[cfg(feature = "ocr-ocrs")]
mod ocrs_backend {
    use super::models_dir;
    use pdfkit_core::{Bitmap, OcrProvider, OcrResult, PdfError};
    use std::path::PathBuf;

    /// Local ONNX OCR via `ocrs` + `rten`, loading `.rten` detection and
    /// recognition models from the cache directory.
    #[derive(Debug, Clone)]
    pub struct OcrsProvider {
        models: PathBuf,
    }

    impl OcrsProvider {
        /// Construct a provider, verifying the detection and recognition models
        /// are present in the cache directory.
        pub fn new() -> Result<Self, PdfError> {
            let models = models_dir();
            let detection = models.join("text-detection.rten");
            let recognition = models.join("text-recognition.rten");
            if !detection.exists() || !recognition.exists() {
                return Err(PdfError::Backend(format!(
                    "ocrs models not found in {}; run scripts/fetch-ocr-models.sh",
                    models.display()
                )));
            }
            Ok(OcrsProvider { models })
        }
    }

    impl OcrProvider for OcrsProvider {
        fn recognize(&self, _bmp: &Bitmap) -> Result<OcrResult, PdfError> {
            // TODO(design): run ocrs (rten) inference with the loaded `.rten`
            // models (load OcrEngine, detect words, recognize text, fill
            // OcrResult/OcrWord with bboxes and confidences). The native lib and
            // models are not available in this build environment, so recognition
            // is not yet wired; the abstraction and pipeline are exercised via a
            // mock provider in pdfkit-core's M5 tests.
            Err(PdfError::Backend(format!(
                "ocrs inference not yet wired (models at {})",
                self.models.display()
            )))
        }
    }
}

#[cfg(feature = "ocr-tesseract")]
mod tesseract_backend {
    use pdfkit_core::{Bitmap, OcrProvider, OcrResult, PdfError};

    /// OCR via the system Tesseract library.
    #[derive(Debug, Clone)]
    pub struct TesseractProvider {
        language: String,
    }

    impl TesseractProvider {
        /// Construct a provider for the given Tesseract language (e.g. `"eng"`).
        pub fn new(language: impl Into<String>) -> Result<Self, PdfError> {
            Ok(TesseractProvider {
                language: language.into(),
            })
        }
    }

    impl Default for TesseractProvider {
        fn default() -> Self {
            TesseractProvider {
                language: "eng".to_string(),
            }
        }
    }

    impl OcrProvider for TesseractProvider {
        fn recognize(&self, _bmp: &Bitmap) -> Result<OcrResult, PdfError> {
            // TODO(design): bind the system Tesseract library and recognize.
            // The library is not available in this build environment.
            Err(PdfError::Backend(format!(
                "tesseract backend ({}) requires the system Tesseract library; not wired",
                self.language
            )))
        }
    }
}

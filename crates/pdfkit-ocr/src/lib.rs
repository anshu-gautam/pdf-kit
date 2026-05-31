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
/// `$XDG_CACHE_HOME/pdfkit/models`, else `~/.cache/pdfkit/models`. Only the
/// ocrs backend loads cached models; Tesseract finds its data via
/// `TESSDATA_PREFIX`.
#[cfg(feature = "ocr-ocrs")]
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
    use ocrs::{ImageSource, OcrEngine, OcrEngineParams, TextItem};
    use pdfkit_core::{Bitmap, OcrProvider, OcrResult, OcrWord, PdfError};

    /// Local ONNX OCR via `ocrs` + `rten`, with the `.rten` detection and
    /// recognition models loaded from the cache directory.
    pub struct OcrsProvider {
        engine: OcrEngine,
    }

    impl std::fmt::Debug for OcrsProvider {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("OcrsProvider").finish_non_exhaustive()
        }
    }

    impl OcrsProvider {
        /// Load the detection and recognition models and build the engine.
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
            let detection_model = rten::Model::load_file(&detection)
                .map_err(|e| PdfError::Backend(format!("ocrs detection model: {e}")))?;
            let recognition_model = rten::Model::load_file(&recognition)
                .map_err(|e| PdfError::Backend(format!("ocrs recognition model: {e}")))?;
            let engine = OcrEngine::new(OcrEngineParams {
                detection_model: Some(detection_model),
                recognition_model: Some(recognition_model),
                ..Default::default()
            })
            .map_err(|e| PdfError::Backend(format!("ocrs engine: {e}")))?;
            Ok(OcrsProvider { engine })
        }
    }

    impl OcrProvider for OcrsProvider {
        fn recognize(&self, bmp: &Bitmap) -> Result<OcrResult, PdfError> {
            // The RGBA bitmap is HWC with 4 channels, which ImageSource accepts.
            let source = ImageSource::from_bytes(&bmp.rgba, (bmp.width, bmp.height))
                .map_err(|e| PdfError::Backend(format!("ocrs image: {e}")))?;
            let input = self
                .engine
                .prepare_input(source)
                .map_err(|e| PdfError::Backend(format!("ocrs prepare: {e}")))?;
            // Full pipeline (detect words -> group into reading-order lines ->
            // recognize) so we can surface per-word bounding boxes, not just the
            // flat text from `get_text`.
            let words = self
                .engine
                .detect_words(&input)
                .map_err(|e| PdfError::Backend(format!("ocrs detect: {e}")))?;
            let line_rects = self.engine.find_text_lines(&input, &words);
            let lines = self
                .engine
                .recognize_text(&input, &line_rects)
                .map_err(|e| PdfError::Backend(format!("ocrs recognize: {e}")))?;

            let mut text = String::new();
            let mut out_words: Vec<OcrWord> = Vec::new();
            for line in lines.into_iter().flatten() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&line.to_string());
                for word in line.words() {
                    let r = word.bounding_rect();
                    out_words.push(OcrWord {
                        text: word.to_string(),
                        bbox: [
                            r.left() as f32,
                            r.top() as f32,
                            r.right() as f32,
                            r.bottom() as f32,
                        ],
                        // ocrs does not expose a per-word confidence; report 1.0
                        // for any recognized word rather than a fabricated score.
                        confidence: 1.0,
                    });
                }
            }
            Ok(OcrResult {
                text,
                confidence: 1.0,
                words: out_words,
            })
        }
    }
}

#[cfg(feature = "ocr-tesseract")]
mod tesseract_backend {
    use leptess::LepTess;
    use pdfkit_core::{encode_png, Bitmap, OcrProvider, OcrResult, PdfError};

    /// OCR via the system Tesseract library (the `leptess` binding). The
    /// language's trained data (e.g. `eng.traineddata`) must be on Tesseract's
    /// `TESSDATA_PREFIX` path.
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
        fn recognize(&self, bmp: &Bitmap) -> Result<OcrResult, PdfError> {
            // Tesseract reads an encoded image; hand it a PNG of the bitmap.
            let png = encode_png(bmp, true)?;
            let mut lep = LepTess::new(None, &self.language).map_err(|e| {
                PdfError::Backend(format!("tesseract init ({}): {e}", self.language))
            })?;
            lep.set_image_from_mem(&png)
                .map_err(|e| PdfError::Backend(format!("tesseract image: {e}")))?;
            let text = lep
                .get_utf8_text()
                .map_err(|e| PdfError::Backend(format!("tesseract recognize: {e}")))?;
            // Tesseract reports a 0..=100 mean word confidence; map to 0.0..=1.0.
            // Per-word boxes aren't surfaced here (a future refinement could use
            // the TSV/hOCR iterator); `words` stays empty.
            let confidence = (lep.mean_text_conf() as f32 / 100.0).clamp(0.0, 1.0);
            Ok(OcrResult {
                text,
                confidence,
                words: Vec::new(),
            })
        }
    }
}

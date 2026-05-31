//! pdfkit-ocr: the re-exported abstraction is usable, and the feature-gated
//! backends report missing models/dependencies clearly.

use pdfkit_core::{Engine, OpenOptions};
use pdfkit_ocr::{ocr_page, Bitmap, NativeRenderer, OcrProvider, OcrResult, PdfError};

struct Mock;
impl OcrProvider for Mock {
    fn recognize(&self, _bmp: &Bitmap) -> Result<OcrResult, PdfError> {
        Ok(OcrResult {
            text: "mock".into(),
            confidence: 1.0,
            words: Vec::new(),
        })
    }
}

#[test]
fn reexported_abstraction_is_usable() {
    let doc = Engine::new()
        .unwrap()
        .open(pdfkit_fixtures::scanned(), OpenOptions::default())
        .unwrap();
    let page = doc.page(1).unwrap();
    let result = ocr_page(&page, &NativeRenderer, &Mock).expect("ocr_page");
    assert_eq!(result.text, "mock");
}

#[cfg(feature = "ocr-ocrs")]
#[test]
fn ocrs_provider_reports_missing_models() {
    // Point at an empty dir so the models are definitely absent.
    std::env::set_var(
        "PDFKIT_OCR_MODELS",
        std::env::temp_dir().join("pdfkit-no-models"),
    );
    let err = pdfkit_ocr::OcrsProvider::new().expect_err("models should be absent");
    assert!(matches!(err, PdfError::Backend(_)), "got {err:?}");
}

#[cfg(feature = "ocr-tesseract")]
#[test]
fn tesseract_provider_recognizes_or_errors_cleanly() {
    // With the system Tesseract + `eng` data present (CI installs them), this
    // runs real OCR on a blank tile and returns Ok with a 0..=1 confidence and
    // no per-word boxes; without them it returns a clean Backend error. Either
    // way it must not panic, and the OcrResult shape must be valid.
    let provider = pdfkit_ocr::TesseractProvider::default();
    let bmp = Bitmap {
        width: 4,
        height: 4,
        rgba: vec![255; 4 * 4 * 4], // white tile
    };
    match provider.recognize(&bmp) {
        Ok(r) => {
            assert!((0.0..=1.0).contains(&r.confidence), "conf {}", r.confidence);
            assert!(r.words.is_empty());
        }
        Err(PdfError::Backend(_)) => {} // libs/data absent: acceptable
        Err(e) => panic!("unexpected error kind: {e:?}"),
    }
}

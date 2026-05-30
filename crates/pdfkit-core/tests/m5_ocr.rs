//! M5 acceptance: the OCR abstraction and its wiring into extraction.
//!
//! A deterministic mock provider stands in for the heavy ONNX backend so the
//! pipeline (render -> recognize -> result, and the extract OCR fallback) is
//! exercised exactly as a real provider would be. The concrete `OcrsProvider`
//! lives in `pdfkit-ocr` behind the `ocr-ocrs` feature.

use pdfkit_core::{
    extract_with_ocr, ocr_page, Bitmap, Engine, ExtractOptions, NativeRenderer, OcrProvider,
    OcrResult, OcrWord, OpenOptions, PdfError,
};

/// A provider that returns fixed text, asserting it received a real bitmap.
struct MockOcr;

impl OcrProvider for MockOcr {
    fn recognize(&self, bmp: &Bitmap) -> Result<OcrResult, PdfError> {
        assert!(
            bmp.width > 0 && bmp.height > 0,
            "provider must receive a rendered bitmap"
        );
        assert_eq!(
            bmp.rgba.len(),
            (bmp.width as usize) * (bmp.height as usize) * 4
        );
        Ok(OcrResult {
            text: "RECOVERED VIA OCR".to_string(),
            confidence: 0.87,
            words: vec![OcrWord {
                text: "RECOVERED".to_string(),
                bbox: [0.0, 0.0, 10.0, 10.0],
                confidence: 0.9,
            }],
        })
    }
}

#[test]
fn ocr_page_renders_then_recognizes() {
    let doc = Engine::new()
        .unwrap()
        .open(pdfkit_fixtures::scanned(), OpenOptions::default())
        .unwrap();
    let page = doc.page(1).unwrap();

    let result = ocr_page(&page, &NativeRenderer, &MockOcr).expect("ocr_page");
    assert_eq!(result.text, "RECOVERED VIA OCR");
    assert!(result.confidence > 0.0 && result.confidence <= 1.0);
    assert_eq!(result.words.len(), 1);
}

#[test]
fn extract_with_ocr_recovers_text_from_scanned_page() {
    let opts = ExtractOptions {
        ocr: true,
        ..Default::default()
    };
    let res =
        extract_with_ocr(pdfkit_fixtures::scanned(), opts, &MockOcr).expect("extract_with_ocr");

    // The scanned page is recovered via OCR -> text present, no image emitted.
    assert!(res.text.contains("RECOVERED VIA OCR"), "got {:?}", res.text);
    assert!(
        res.images.is_empty(),
        "OCR'd pages should not also be rendered"
    );
    assert!(!res.truncated.images);
}

#[test]
fn extract_with_ocr_off_falls_back_to_render() {
    let opts = ExtractOptions {
        ocr: false,
        ..Default::default()
    };
    let res = extract_with_ocr(pdfkit_fixtures::scanned(), opts, &MockOcr).expect("extract");
    // OCR disabled -> scanned page is rendered to an image, not recognized.
    assert!(!res.text.contains("RECOVERED"));
    assert_eq!(res.images.len(), 1);
}

#[test]
fn extract_with_ocr_on_text_pdf_returns_text_only() {
    let opts = ExtractOptions {
        ocr: true,
        ..Default::default()
    };
    let res = extract_with_ocr(pdfkit_fixtures::born_digital(), opts, &MockOcr).expect("extract");
    // Plenty of real text -> provider is never consulted.
    assert!(res.text.contains("Hello, pdfkit!"));
    assert!(!res.text.contains("RECOVERED"));
    assert!(res.images.is_empty());
}

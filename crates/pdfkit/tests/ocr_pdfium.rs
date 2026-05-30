//! End-to-end OCR verification: render a text page with PDFIUM, then recognize
//! it with the real ocrs engine. Requires the PDFIUM library and the ocrs models
//! to be installed, so it is #[ignore]d by default. Run with:
//!
//!   cargo test -p pdfkit --features render-pdfium,ocr-ocrs --test ocr_pdfium -- --ignored --nocapture
#![cfg(all(feature = "render-pdfium", feature = "ocr-ocrs"))]

use pdfkit::{OcrProvider, OcrsProvider, PdfiumRenderer, RenderOptions};

#[test]
#[ignore = "requires the PDFIUM library and ocrs models"]
fn ocr_reads_a_pdfium_rendered_page() {
    let bytes = pdfkit_fixtures::born_digital();

    let renderer = PdfiumRenderer::new().expect("bind PDFIUM");
    let opts = RenderOptions {
        dpi: Some(300.0),
        max_pixels: 40_000_000,
        ..Default::default()
    };
    let bitmap = renderer
        .render_page(&bytes, 1, None, &opts)
        .expect("render");

    let provider = OcrsProvider::new().expect("load ocrs models");
    let result = provider.recognize(&bitmap).expect("ocr");

    eprintln!("--- OCR TEXT ---\n{}\n----------------", result.text);
    let lower = result.text.to_lowercase();
    assert!(!lower.trim().is_empty(), "OCR should recover some text");
    assert!(
        ["hello", "pdfkit", "born", "fox", "quartz"]
            .iter()
            .any(|w| lower.contains(w)),
        "expected a recognizable word, got: {:?}",
        result.text
    );
}

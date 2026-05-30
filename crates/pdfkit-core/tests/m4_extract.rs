//! M4 acceptance: the `extract` entry point and its four modes.

use pdfkit_core::{extract, ExtractOptions, Mode, RenderOptions};

#[test]
fn text_pdf_auto_returns_text_only() {
    let res = extract(pdfkit_fixtures::born_digital(), ExtractOptions::default()).expect("extract");
    assert!(res.text.contains("Hello, pdfkit!"));
    assert!(res.images.is_empty(), "text PDF must not produce images");
    assert_eq!(res.pages_processed, vec![1]);
    assert!(!res.truncated.text);
    assert!(!res.truncated.images);
}

#[test]
fn scanned_pdf_auto_renders_images_when_ocr_off() {
    let opts = ExtractOptions {
        ocr: false,
        ..Default::default()
    };
    let res = extract(pdfkit_fixtures::scanned(), opts).expect("extract");
    // Scanned page has ~no text, so Auto falls back to a rendered PNG.
    assert_eq!(res.images.len(), 1, "expected one rendered page image");
    assert_eq!(res.images[0].page, 1);
    assert!(res.images[0].png.starts_with(&[137, 80, 78, 71]));
    assert!(res.images[0].width > 0 && res.images[0].height > 0);
}

#[test]
fn mode_text_never_renders() {
    let opts = ExtractOptions {
        mode: Mode::Text,
        ..Default::default()
    };
    let res = extract(pdfkit_fixtures::scanned(), opts).expect("extract");
    assert!(res.images.is_empty());
}

#[test]
fn mode_images_renders_all_selected_pages() {
    let opts = ExtractOptions {
        mode: Mode::Images,
        ..Default::default()
    };
    let res = extract(pdfkit_fixtures::born_digital(), opts).expect("extract");
    assert_eq!(res.images.len(), 1);
    assert!(res.text.is_empty());
}

#[test]
fn mode_both_returns_text_and_images() {
    let opts = ExtractOptions {
        mode: Mode::Both,
        ..Default::default()
    };
    let res = extract(pdfkit_fixtures::mixed(), opts).expect("extract");
    assert!(res.text.contains("mixes a real text layer"));
    assert_eq!(res.images.len(), 1);
}

#[test]
fn tight_pixel_budget_truncates_images() {
    let opts = ExtractOptions {
        mode: Mode::Auto,
        image: RenderOptions {
            max_pixels: 100, // far too small for any real page
            ..Default::default()
        },
        ..Default::default()
    };
    let res = extract(pdfkit_fixtures::scanned(), opts).expect("extract");
    assert!(res.images.is_empty(), "nothing should fit the budget");
    assert!(res.truncated.images, "truncation flag must be set");
}

#[test]
fn max_text_chars_truncates_text() {
    let opts = ExtractOptions {
        mode: Mode::Text,
        max_text_chars: 5,
        ..Default::default()
    };
    let res = extract(pdfkit_fixtures::born_digital(), opts).expect("extract");
    assert_eq!(res.text.chars().count(), 5);
    assert!(res.truncated.text);
}

#[test]
fn page_selection_is_respected() {
    let opts = ExtractOptions {
        pages: Some(vec![1]),
        ..Default::default()
    };
    let res = extract(pdfkit_fixtures::born_digital(), opts).expect("extract");
    assert_eq!(res.pages_processed, vec![1]);
}

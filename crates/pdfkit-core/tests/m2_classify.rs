//! M2 acceptance: page classification.
//! born-digital -> TextBased, scanned -> Scanned, mixed -> Mixed.

use pdfkit_core::{Engine, OpenOptions, PageKind};

fn classify_first(bytes: Vec<u8>) -> (PageKind, pdfkit_core::PageSignals) {
    let doc = Engine::new()
        .expect("engine")
        .open(bytes, OpenOptions::default())
        .expect("open");
    let page = doc.page(1).expect("page 1");
    (page.classify(), page.signals())
}

#[test]
fn born_digital_is_text_based() {
    let (kind, sig) = classify_first(pdfkit_fixtures::born_digital());
    assert_eq!(kind, PageKind::TextBased, "signals: {sig:?}");
    assert!(sig.text_char_count > 0);
    assert!(sig.image_coverage < 0.01, "no images expected: {sig:?}");
}

#[test]
fn scanned_is_scanned() {
    let (kind, sig) = classify_first(pdfkit_fixtures::scanned());
    assert_eq!(kind, PageKind::Scanned, "signals: {sig:?}");
    assert_eq!(sig.image_count, 1);
    assert!(
        sig.image_coverage > 0.9,
        "full-page image expected: {sig:?}"
    );
    assert_eq!(sig.text_char_count, 0);
}

#[test]
fn mixed_is_mixed() {
    let (kind, sig) = classify_first(pdfkit_fixtures::mixed());
    assert_eq!(kind, PageKind::Mixed, "signals: {sig:?}");
    assert!(sig.text_char_count > 0, "text expected: {sig:?}");
    assert!(
        sig.image_coverage >= 0.5,
        "substantial image expected: {sig:?}"
    );
}

//! M1 acceptance: open (path + bytes), metadata, page count, text extraction,
//! and the encrypted/wrong-password path.

use pdfkit_core::{Engine, OpenOptions, PdfError, TextOptions};

fn engine() -> Engine {
    Engine::new().expect("engine")
}

#[test]
fn opens_bytes_and_reads_metadata() {
    let doc = engine()
        .open(pdfkit_fixtures::born_digital(), OpenOptions::default())
        .expect("open born-digital from bytes");

    assert_eq!(doc.page_count(), 1);
    let md = doc.metadata();
    assert_eq!(md.page_count, 1);
    assert_eq!(md.title.as_deref(), Some("Born Digital Fixture"));
    assert_eq!(md.author.as_deref(), Some("pdfkit"));
    assert_eq!(md.pdf_version, "1.5");
    assert!(!md.encrypted);
}

#[test]
fn opens_from_path() {
    // Write the fixture to a temp file to exercise the Path input branch.
    let mut path = std::env::temp_dir();
    path.push("pdfkit-m1-born-digital.pdf");
    std::fs::write(&path, pdfkit_fixtures::born_digital()).expect("write temp pdf");

    let doc = engine()
        .open(&path, OpenOptions::default())
        .expect("open from path");
    assert_eq!(doc.page_count(), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn extracts_text_layer() {
    let doc = engine()
        .open(pdfkit_fixtures::born_digital(), OpenOptions::default())
        .expect("open");

    let text = doc.text(TextOptions::default()).expect("extract text");

    for line in pdfkit_fixtures::BORN_DIGITAL_LINES {
        assert!(
            text.contains(line),
            "extracted text missing line: {line:?}\ngot: {text:?}"
        );
    }

    let page_text = doc.page(1).expect("page 1").text().expect("page text");
    assert!(page_text.contains("Hello, pdfkit!"));
}

#[test]
fn page_geometry_and_bounds() {
    let doc = engine()
        .open(pdfkit_fixtures::born_digital(), OpenOptions::default())
        .expect("open");

    let page = doc.page(1).expect("page 1");
    assert_eq!(page.number(), 1);
    let (w, h) = page.size_points();
    assert!((w - 612.0).abs() < 0.5, "width {w}");
    assert!((h - 792.0).abs() < 0.5, "height {h}");
    assert_eq!(page.rotation(), 0);

    // Out-of-range (and zero) page numbers are PageRange errors, not panics.
    assert!(matches!(doc.page(0), Err(PdfError::PageRange(0))));
    assert!(matches!(doc.page(2), Err(PdfError::PageRange(2))));
}

#[test]
fn text_options_respect_max_chars() {
    let doc = engine()
        .open(pdfkit_fixtures::born_digital(), OpenOptions::default())
        .expect("open");
    let opts = TextOptions {
        max_chars: 5,
        ..Default::default()
    };
    let text = doc.text(opts).expect("extract");
    assert_eq!(text.chars().count(), 5);
}

#[test]
fn wrong_password_is_password_error() {
    let bytes = pdfkit_fixtures::encrypted_default();
    let err = engine()
        .open(bytes, OpenOptions::with_password("definitely-wrong"))
        .expect_err("wrong password must fail");
    assert!(matches!(err, PdfError::Password), "got {err:?}");
}

#[test]
fn missing_password_on_encrypted_is_password_error() {
    let bytes = pdfkit_fixtures::encrypted_default();
    let err = engine()
        .open(bytes, OpenOptions::default())
        .expect_err("missing password must fail");
    assert!(matches!(err, PdfError::Password), "got {err:?}");
}

#[test]
fn correct_password_opens_encrypted() {
    let bytes = pdfkit_fixtures::encrypted_default();
    let doc = engine()
        .open(
            bytes,
            OpenOptions::with_password(pdfkit_fixtures::ENCRYPTED_USER_PASSWORD),
        )
        .expect("correct password opens");
    assert!(doc.metadata().encrypted);
    let text = doc.text(TextOptions::default()).expect("text");
    assert!(text.contains("encrypted"), "got {text:?}");
}

#[test]
fn oversized_xref_stream_width_is_rejected_before_backend_allocation() {
    let mut mutated = pdfkit_fixtures::multi_heading();
    let needle = b"/W[1 4 2]";
    let pos = mutated
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("fixture has xref stream width array");
    mutated.splice(
        pos..pos + needle.len(),
        b"/W[1 4777777777777777 2]".iter().copied(),
    );

    let err = engine()
        .open(mutated, OpenOptions::default())
        .expect_err("malformed xref stream width must fail before allocation");

    assert!(matches!(err, PdfError::Format(_)), "got {err:?}");
}

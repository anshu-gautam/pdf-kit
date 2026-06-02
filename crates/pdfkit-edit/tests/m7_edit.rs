//! M7 acceptance: create -> reopen -> read text; merge page-count; split ranges.
//! Plus remove/rotate/watermark/fill_form coverage.

use std::collections::HashMap;

use pdfkit_core::{Engine, OpenOptions, TextOptions};
use pdfkit_edit::{FontSpec, PageSize, PdfBuilder, PdfEditor, WatermarkOptions};

fn open_text(bytes: &[u8]) -> String {
    let doc = Engine::new()
        .unwrap()
        .open(bytes.to_vec(), OpenOptions::default())
        .unwrap();
    doc.text(TextOptions::default()).unwrap()
}

fn open_page_count(bytes: &[u8]) -> usize {
    Engine::new()
        .unwrap()
        .open(bytes.to_vec(), OpenOptions::default())
        .unwrap()
        .page_count()
}

fn three_page_doc() -> Vec<u8> {
    let mut b = PdfBuilder::new();
    for i in 1..=3 {
        let p = b.add_page(PageSize::Letter);
        b.draw_text(
            p,
            &format!("Page number {i} body"),
            (72.0, 700.0),
            FontSpec::default(),
        );
    }
    let mut out = Vec::new();
    b.save(&mut out).unwrap();
    out
}

#[test]
fn create_then_reopen_reads_text_back() {
    let mut b = PdfBuilder::new();
    let p = b.add_page(PageSize::Letter);
    b.draw_text(
        p,
        "Round trip text here",
        (72.0, 720.0),
        FontSpec::default(),
    );
    let mut out = Vec::new();
    b.save(&mut out).expect("save");

    assert_eq!(open_page_count(&out), 1);
    assert!(
        open_text(&out).contains("Round trip text here"),
        "got {:?}",
        open_text(&out)
    );
}

#[test]
fn place_image_roundtrips() {
    // Encode a small PNG and place it.
    let mut img = image::RgbImage::new(8, 8);
    for px in img.pixels_mut() {
        *px = image::Rgb([10, 200, 30]);
    }
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();

    let mut b = PdfBuilder::new();
    let p = b.add_page(PageSize::A4);
    b.place_image(p, &png, [100.0, 100.0, 300.0, 300.0])
        .expect("place image");
    let mut out = Vec::new();
    b.save(&mut out).expect("save");
    assert_eq!(open_page_count(&out), 1);
}

#[test]
fn merge_combines_page_counts() {
    let mut a = PdfEditor::open(pdfkit_fixtures::born_digital()).expect("open a");
    let b = PdfEditor::open(pdfkit_fixtures::scanned()).expect("open b");
    assert_eq!(a.page_count(), 1);
    assert_eq!(b.page_count(), 1);

    a.merge(&b).expect("merge");
    assert_eq!(a.page_count(), 2);

    let mut out = Vec::new();
    a.save(&mut out).expect("save");
    assert_eq!(open_page_count(&out), 2);
}

#[test]
fn split_produces_expected_ranges() {
    let doc = three_page_doc();
    let editor = PdfEditor::open(doc).expect("open");
    assert_eq!(editor.page_count(), 3);

    let parts = editor.split(&[(1, 1), (2, 3)]).expect("split");
    assert_eq!(parts.len(), 2);
    assert_eq!(open_page_count(&parts[0]), 1);
    assert_eq!(open_page_count(&parts[1]), 2);

    // The first part keeps page 1's text only.
    assert!(open_text(&parts[0]).contains("Page number 1"));
    assert!(!open_text(&parts[0]).contains("Page number 2"));
    // The second part keeps pages 2 and 3.
    let t = open_text(&parts[1]);
    assert!(
        t.contains("Page number 2") && t.contains("Page number 3"),
        "got {t:?}"
    );
}

#[test]
fn split_rejects_bad_ranges() {
    let editor = PdfEditor::open(three_page_doc()).expect("open");
    assert!(editor.split(&[(0, 1)]).is_err());
    assert!(editor.split(&[(2, 1)]).is_err());
    assert!(editor.split(&[(1, 9)]).is_err());
}

#[test]
fn remove_pages_drops_them() {
    let mut editor = PdfEditor::open(three_page_doc()).expect("open");
    editor.remove_pages(&[2]).expect("remove");
    assert_eq!(editor.page_count(), 2);
    let mut out = Vec::new();
    editor.save(&mut out).expect("save");
    let t = open_text(&out);
    assert!(t.contains("Page number 1") && t.contains("Page number 3"));
    assert!(!t.contains("Page number 2"), "page 2 should be gone: {t:?}");
}

#[test]
fn rotate_page_sets_rotation() {
    let mut editor = PdfEditor::open(pdfkit_fixtures::born_digital()).expect("open");
    editor.rotate_page(1, 90).expect("rotate");
    let mut out = Vec::new();
    editor.save(&mut out).expect("save");

    let doc = Engine::new()
        .unwrap()
        .open(out, OpenOptions::default())
        .unwrap();
    assert_eq!(doc.page(1).unwrap().rotation(), 90);
}

#[test]
fn watermark_adds_visible_text() {
    let mut editor = PdfEditor::open(pdfkit_fixtures::born_digital()).expect("open");
    editor
        .watermark("CONFIDENTIAL", WatermarkOptions::default())
        .expect("watermark");
    let mut out = Vec::new();
    editor.save(&mut out).expect("save");
    assert!(
        open_text(&out).contains("CONFIDENTIAL"),
        "watermark text should be present"
    );
}

#[test]
fn watermark_preserves_original_page_content() {
    // born-digital inherits its /Resources from the page tree. Watermarking must
    // not shadow them with a fresh dict holding only the watermark font, or the
    // original text loses its font and renders blank (regression guard).
    let mut editor = PdfEditor::open(pdfkit_fixtures::born_digital()).expect("open");
    editor
        .watermark("DRAFT", WatermarkOptions::default())
        .expect("watermark");
    let mut out = Vec::new();
    editor.save(&mut out).expect("save");

    // The original text must still be readable, alongside the watermark.
    let text = open_text(&out);
    assert!(
        text.contains("Hello, pdfkit!"),
        "original text lost: {text:?}"
    );
    assert!(text.contains("DRAFT"), "watermark text missing: {text:?}");

    // The page's resolved /Font resources must carry the original font(s) in
    // addition to the watermark font — not just the watermark.
    let doc = lopdf::Document::load_mem(&out).expect("reopen");
    let (_, page_id) = doc.get_pages().into_iter().next().expect("a page");
    let resources_id = doc
        .get_dictionary(page_id)
        .unwrap()
        .get(b"Resources")
        .unwrap()
        .as_reference()
        .unwrap();
    let fonts = doc
        .get_dictionary(resources_id)
        .unwrap()
        .get(b"Font")
        .unwrap()
        .as_dict()
        .unwrap();
    let keys: Vec<String> = fonts
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
        .collect();
    assert!(
        keys.iter().any(|k| k == "PDFKitWM"),
        "watermark font missing: {keys:?}"
    );
    assert!(
        keys.iter().any(|k| k != "PDFKitWM"),
        "original font dropped: {keys:?}"
    );
}

#[test]
fn fill_form_sets_field_value() {
    let mut editor = PdfEditor::open(pdfkit_fixtures::forms()).expect("open");
    assert_eq!(
        editor
            .form_field_value(pdfkit_fixtures::FORM_FIELD_NAME)
            .as_deref(),
        Some("")
    );

    let mut fields = HashMap::new();
    fields.insert(
        pdfkit_fixtures::FORM_FIELD_NAME.to_string(),
        "Ada Lovelace".to_string(),
    );
    editor.fill_form(&fields).expect("fill");

    let mut out = Vec::new();
    editor.save(&mut out).expect("save");

    let reopened = PdfEditor::open(out).expect("reopen");
    assert_eq!(
        reopened
            .form_field_value(pdfkit_fixtures::FORM_FIELD_NAME)
            .as_deref(),
        Some("Ada Lovelace")
    );
}

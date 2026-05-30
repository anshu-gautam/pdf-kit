//! Deterministic, synthetic PDF fixtures for the pdfkit test suite.
//!
//! Every fixture is generated from code with `lopdf` so the bytes are tiny,
//! reproducible, and committable. Functions return owned `Vec<u8>` buffers;
//! `write-fixtures` dumps them into the workspace `fixtures/` directory.
//!
//! This is internal test tooling (`publish = false`), so the generators use
//! `expect` on what are effectively compile-time-correct construction steps.

use lopdf::content::{Content, Operation};
use lopdf::{
    dictionary, Dictionary, Document, EncryptionState, EncryptionVersion, Object, Permissions,
    Stream,
};

/// US Letter page size in points.
const PAGE_W: i64 = 612;
const PAGE_H: i64 = 792;

/// The owner/user passwords baked into [`encrypted`] and [`encrypted_default`].
pub const ENCRYPTED_OWNER_PASSWORD: &str = "owner-secret";
/// The correct user password for the encrypted fixture.
pub const ENCRYPTED_USER_PASSWORD: &str = "open-sesame";

/// The exact text lines drawn into [`born_digital`], in order. There are enough
/// of them to clear the default `min_text_chars` (200) so the document reads as
/// genuinely text-based.
pub const BORN_DIGITAL_LINES: &[&str] = &[
    "Hello, pdfkit!",
    "This is a born-digital fixture.",
    "It carries a real text layer.",
    "The quick brown fox jumps over the lazy dog.",
    "Pack my box with five dozen liquor jugs.",
    "Sphinx of black quartz, judge my vow.",
    "How vexingly quick daft zebras jump!",
    "The five boxing wizards jump quickly.",
];

/// A single-page, born-digital PDF with a real text layer plus Title/Author.
pub fn born_digital() -> Vec<u8> {
    let ops = text_ops(BORN_DIGITAL_LINES);
    to_bytes(assemble("Born Digital Fixture", "pdfkit", ops, vec![]))
}

/// A scanned page: one full-page image and no text layer.
pub fn scanned() -> Vec<u8> {
    let ops = image_ops("Im0", PAGE_W, PAGE_H, 0, 0);
    let images = vec![("Im0", gray_image(4, 4))];
    to_bytes(assemble("Scanned Fixture", "pdfkit", ops, images))
}

/// A mixed page: a substantial embedded image *and* a real text layer.
pub fn mixed() -> Vec<u8> {
    let lines = [
        "This page mixes a real text layer",
        "with a large embedded image above it.",
    ];
    let mut ops = image_ops("Im0", PAGE_W, 542, 0, 250); // ~68% of the page
    ops.extend(text_ops(&lines));
    let images = vec![("Im0", gray_image(4, 4))];
    to_bytes(assemble("Mixed Fixture", "pdfkit", ops, images))
}

/// Lines of the multi-heading fixture as `(text, font_size_points)`, in order.
/// Two heading levels (22 and 16 pt) over 11 pt body text.
pub const MULTI_HEADING_LINES: &[(&str, i64)] = &[
    ("Chapter One", 22),
    ("Section A", 16),
    (
        "Alpha body paragraph with some descriptive sentence text here.",
        11,
    ),
    (
        "More alpha body text to give the section a little extra volume.",
        11,
    ),
    ("Section B", 16),
    (
        "Beta body paragraph describing the second section in brief here.",
        11,
    ),
    ("Chapter Two", 22),
    (
        "Gamma body paragraph sitting under the second chapter heading now.",
        11,
    ),
    // An empty line is a paragraph spacer: it forces a block break so Gamma and
    // Delta are separate blocks that the packer then recombines under target.
    ("", 11),
    (
        "Delta paragraph is a separate block under the same chapter heading.",
        11,
    ),
];

/// A document with two heading levels and body paragraphs, for chunk tests.
pub fn multi_heading() -> Vec<u8> {
    let ops = sized_text_ops(MULTI_HEADING_LINES);
    to_bytes(assemble("Multi Heading Fixture", "pdfkit", ops, vec![]))
}

/// The text field name in the [`forms`] fixture.
pub const FORM_FIELD_NAME: &str = "name";

/// A single-page PDF with an AcroForm containing one text field named
/// [`FORM_FIELD_NAME`].
pub fn forms() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();

    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "Helv" => font_id },
    });

    let field_id = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Tx",
        "T" => Object::string_literal(FORM_FIELD_NAME),
        "V" => Object::string_literal(""),
        "Rect" => vec![100_i64.into(), 700_i64.into(), 300_i64.into(), 720_i64.into()],
        "P" => page_id,
        "DA" => Object::string_literal("/Helv 12 Tf 0 g"),
    });

    doc.objects.insert(
        page_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0_i64.into(), 0_i64.into(), PAGE_W.into(), PAGE_H.into()],
            "Resources" => resources_id,
            "Annots" => vec![field_id.into()],
        }),
    );
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1_i64,
        }),
    );

    let acroform_id = doc.add_object(dictionary! {
        "Fields" => vec![field_id.into()],
        "NeedAppearances" => true,
    });
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "AcroForm" => acroform_id,
    });
    doc.trailer.set("Root", catalog_id);

    let file_id = Object::string_literal(&b"pdfkit-fixture01"[..]);
    doc.trailer
        .set("ID", Object::Array(vec![file_id.clone(), file_id]));

    to_bytes(doc)
}

/// The born-digital document encrypted (RC4-40, V1) with the well-known
/// owner/user passwords above.
pub fn encrypted_default() -> Vec<u8> {
    encrypted(ENCRYPTED_OWNER_PASSWORD, ENCRYPTED_USER_PASSWORD)
}

/// An encrypted PDF using the given owner/user passwords.
pub fn encrypted(owner: &str, user: &str) -> Vec<u8> {
    let ops = text_ops(&["This document is encrypted.", "The secret is safe."]);
    let mut doc = assemble("Encrypted Fixture", "pdfkit", ops, vec![]);
    let version = EncryptionVersion::V1 {
        document: &doc,
        owner_password: owner,
        user_password: user,
        permissions: Permissions::all(),
    };
    let state = EncryptionState::try_from(version).expect("derive encryption state");
    doc.encrypt(&state).expect("encrypt document");
    to_bytes(doc)
}

/// Text-drawing operations: one `BT..ET` block, absolute first line, then line
/// breaks. Empty when there are no lines.
fn text_ops(lines: &[&str]) -> Vec<Operation> {
    if lines.is_empty() {
        return Vec::new();
    }
    let mut ops = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F1".into(), 14_i64.into()]),
        Operation::new("Td", vec![72_i64.into(), 740_i64.into()]),
    ];
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            ops.push(Operation::new("Td", vec![0_i64.into(), (-18_i64).into()]));
        }
        ops.push(Operation::new("Tj", vec![Object::string_literal(*line)]));
    }
    ops.push(Operation::new("ET", vec![]));
    ops
}

/// Text operations drawing each `(line, size)` at an absolute position, one per
/// line, top to bottom, with per-line font size.
fn sized_text_ops(lines: &[(&str, i64)]) -> Vec<Operation> {
    let mut ops = vec![Operation::new("BT", vec![])];
    let mut y = 740.0f32;
    for (text, size) in lines {
        let s = *size as f32;
        if text.is_empty() {
            // Paragraph spacer: advance the cursor without drawing.
            y -= s * 1.6 + 6.0;
            continue;
        }
        ops.push(Operation::new("Tf", vec!["F1".into(), (*size).into()]));
        ops.push(Operation::new(
            "Tm",
            vec![
                1.0f32.into(),
                0.0f32.into(),
                0.0f32.into(),
                1.0f32.into(),
                72.0f32.into(),
                y.into(),
            ],
        ));
        ops.push(Operation::new("Tj", vec![Object::string_literal(*text)]));
        y -= s * 1.6 + 6.0;
    }
    ops.push(Operation::new("ET", vec![]));
    ops
}

/// Operations that paint image XObject `name` into the rectangle described by a
/// `cm` of `[w 0 0 h x y]` (drawn area = w*h points).
fn image_ops(name: &str, w: i64, h: i64, x: i64, y: i64) -> Vec<Operation> {
    vec![
        Operation::new("q", vec![]),
        Operation::new(
            "cm",
            vec![
                w.into(),
                0_i64.into(),
                0_i64.into(),
                h.into(),
                x.into(),
                y.into(),
            ],
        ),
        Operation::new("Do", vec![name.into()]),
        Operation::new("Q", vec![]),
    ]
}

/// A small mid-gray image XObject (`w*h` bytes, DeviceGray, 8 bpc). Pixel size
/// is irrelevant to coverage — the `cm` transform sets the drawn area.
fn gray_image(w: i64, h: i64) -> Stream {
    let data = vec![160u8; (w * h) as usize];
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => w,
            "Height" => h,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8_i64,
        },
        data,
    )
}

/// Assemble a one-page document from content operations and named image
/// XObjects, recording Title/Author and a fixed document /ID.
fn assemble(
    title: &str,
    author: &str,
    ops: Vec<Operation>,
    images: Vec<(&str, Stream)>,
) -> Document {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });

    let has_images = !images.is_empty();
    let mut xobjects = Dictionary::new();
    for (name, stream) in images {
        let id = doc.add_object(stream);
        xobjects.set(name, id);
    }

    let mut resources = dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    };
    if has_images {
        resources.set("XObject", xobjects);
    }
    let resources_id = doc.add_object(resources);

    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        Content { operations: ops }
            .encode()
            .expect("encode content stream"),
    ));

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "MediaBox" => vec![0_i64.into(), 0_i64.into(), PAGE_W.into(), PAGE_H.into()],
    });

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1_i64,
        "Resources" => resources_id,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let info_id = doc.add_object(dictionary! {
        "Title" => Object::string_literal(title),
        "Author" => Object::string_literal(author),
    });
    doc.trailer.set("Info", info_id);

    let file_id = Object::string_literal(&b"pdfkit-fixture01"[..]);
    doc.trailer
        .set("ID", Object::Array(vec![file_id.clone(), file_id]));

    doc
}

fn to_bytes(mut doc: Document) -> Vec<u8> {
    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("serialize document");
    buf
}

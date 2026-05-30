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
    dictionary, Document, EncryptionState, EncryptionVersion, Object, Permissions, Stream,
};

/// The owner/user passwords baked into [`encrypted`] and [`encrypted_default`].
pub const ENCRYPTED_OWNER_PASSWORD: &str = "owner-secret";
/// The correct user password for the encrypted fixture.
pub const ENCRYPTED_USER_PASSWORD: &str = "open-sesame";

/// The exact text lines drawn into [`born_digital`], in order.
pub const BORN_DIGITAL_LINES: &[&str] = &[
    "Hello, pdfkit!",
    "This is a born-digital fixture.",
    "It carries a real text layer.",
];

/// A single-page, born-digital PDF with a real text layer plus Title/Author.
pub fn born_digital() -> Vec<u8> {
    let doc = build_text_doc("Born Digital Fixture", "pdfkit", BORN_DIGITAL_LINES);
    to_bytes(doc)
}

/// The born-digital document encrypted (RC4-40, V1) with the well-known
/// owner/user passwords above.
pub fn encrypted_default() -> Vec<u8> {
    encrypted(ENCRYPTED_OWNER_PASSWORD, ENCRYPTED_USER_PASSWORD)
}

/// An encrypted PDF using the given owner/user passwords.
pub fn encrypted(owner: &str, user: &str) -> Vec<u8> {
    let mut doc = build_text_doc(
        "Encrypted Fixture",
        "pdfkit",
        &["This document is encrypted.", "The secret is safe."],
    );
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

/// Build a one-page document that draws `lines` of text and records `title` /
/// `author` in the information dictionary.
fn build_text_doc(title: &str, author: &str, lines: &[&str]) -> Document {
    let mut doc = Document::with_version("1.5");

    // Reserve the Pages node id up front so the page can point at its parent.
    let pages_id = doc.new_object_id();

    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });

    // Content stream: one BT..ET block, absolute first line, then line breaks.
    let mut ops = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F1".into(), 14_i64.into()]),
        Operation::new("Td", vec![72_i64.into(), 740_i64.into()]),
    ];
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            // Move down one line (relative to the previous line origin).
            ops.push(Operation::new("Td", vec![0_i64.into(), (-18_i64).into()]));
        }
        ops.push(Operation::new("Tj", vec![Object::string_literal(*line)]));
    }
    ops.push(Operation::new("ET", vec![]));

    let content = Content { operations: ops };
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        content.encode().expect("encode content stream"),
    ));

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "MediaBox" => vec![0_i64.into(), 0_i64.into(), 612_i64.into(), 792_i64.into()],
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

    // A document /ID (two 16-byte strings) is required by the encryption
    // handler and is good practice generally. Keep it fixed for determinism.
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

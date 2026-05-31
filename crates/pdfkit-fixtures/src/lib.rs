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
    Stream, StringFormat,
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

/// A page with a 3-column x 3-row text table (cells placed at fixed column x
/// positions) followed by a "Figure 1: ..." caption line. For chunk
/// table/caption detection.
pub fn table_doc() -> Vec<u8> {
    // (text, x, y): three columns at x = 72 / 250 / 430, three rows.
    let cells: &[(&str, i64, i64)] = &[
        ("Name", 72, 700),
        ("Role", 250, 700),
        ("Level", 430, 700),
        ("Ada", 72, 680),
        ("Engineering", 250, 680),
        ("Senior", 430, 680),
        ("Linus", 72, 660),
        ("Systems", 250, 660),
        ("Staff", 430, 660),
    ];
    let mut ops = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F1".into(), 11_i64.into()]),
    ];
    let place = |ops: &mut Vec<Operation>, text: &str, x: i64, y: i64| {
        ops.push(Operation::new(
            "Tm",
            vec![
                1.0f32.into(),
                0.0f32.into(),
                0.0f32.into(),
                1.0f32.into(),
                (x as f32).into(),
                (y as f32).into(),
            ],
        ));
        ops.push(Operation::new("Tj", vec![Object::string_literal(text)]));
    };
    for (text, x, y) in cells {
        place(&mut ops, text, *x, *y);
    }
    place(&mut ops, "Figure 1: Team roster table.", 72, 630);
    ops.push(Operation::new("ET", vec![]));
    to_bytes(assemble("Table Fixture", "pdfkit", ops, vec![]))
}

/// Expected total advance width, in points, of the single show-text run in
/// [`type0_identity`]: CID 0 (2000) + CID 1 (1000) = 3000/1000 em at 10pt = 30pt.
pub const TYPE0_ADVANCE_PTS: f32 = 30.0;

/// A single-page PDF whose only font is a composite Type0 font with an
/// `Identity-H` encoding and a descendant CIDFont carrying explicit `/W`
/// per-CID widths (CID 0 = 2000, CID 1 = 1000) plus `/DW` 1000. The content
/// shows the two-byte codes `0x0000 0x0001`, so a correct CID-aware advance is
/// [`TYPE0_ADVANCE_PTS`] — unlike the old char-count×0.5em estimate.
pub fn type0_identity() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    let descriptor_id = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "PKID+Test",
        "Flags" => 4_i64,
        "ItalicAngle" => 0_i64,
        "Ascent" => 800_i64,
        "Descent" => -200_i64,
        "CapHeight" => 700_i64,
        "StemV" => 80_i64,
        "FontBBox" => vec![0_i64.into(), (-200_i64).into(), 1000_i64.into(), 800_i64.into()],
    });
    let cidfont_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "PKID+Test",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0_i64,
        },
        "FontDescriptor" => descriptor_id,
        "DW" => 1000_i64,
        // /W form `c [w0 w1]`: CID 0 -> 2000, CID 1 -> 1000.
        "W" => vec![0_i64.into(), vec![2000_i64.into(), 1000_i64.into()].into()],
        "CIDToGIDMap" => "Identity",
    });
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "PKID+Test",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![cidfont_id.into()],
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F0" => font_id },
    });

    let ops = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F0".into(), 10_i64.into()]),
        Operation::new(
            "Tm",
            vec![
                1.0f32.into(),
                0.0f32.into(),
                0.0f32.into(),
                1.0f32.into(),
                100.0f32.into(),
                700.0f32.into(),
            ],
        ),
        Operation::new(
            "Tj",
            vec![Object::String(
                vec![0x00, 0x00, 0x00, 0x01],
                StringFormat::Hexadecimal,
            )],
        ),
        Operation::new("ET", vec![]),
    ];
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
        "Resources" => resources_id,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let file_id = Object::string_literal(&b"pdfkit-fixture01"[..]);
    doc.trailer
        .set("ID", Object::Array(vec![file_id.clone(), file_id]));
    to_bytes(doc)
}

/// Two separate 2x2 text tables stacked with a large vertical gap (so they form
/// distinct blocks), for verifying that stacked tables stay separate chunks.
pub fn two_tables() -> Vec<u8> {
    // (text, x, y): two columns at x=72/250; table 1 at y=700/680, table 2 at
    // y=600/580 (the 80pt gap exceeds the block-merge threshold).
    let cells: &[(&str, i64, i64)] = &[
        ("A1", 72, 700),
        ("B1", 250, 700),
        ("A2", 72, 680),
        ("B2", 250, 680),
        ("C1", 72, 600),
        ("D1", 250, 600),
        ("C2", 72, 580),
        ("D2", 250, 580),
    ];
    let mut ops = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F1".into(), 11_i64.into()]),
    ];
    for (text, x, y) in cells {
        ops.push(Operation::new(
            "Tm",
            vec![
                1.0f32.into(),
                0.0f32.into(),
                0.0f32.into(),
                1.0f32.into(),
                (*x as f32).into(),
                (*y as f32).into(),
            ],
        ));
        ops.push(Operation::new("Tj", vec![Object::string_literal(*text)]));
    }
    ops.push(Operation::new("ET", vec![]));
    to_bytes(assemble("Two Tables Fixture", "pdfkit", ops, vec![]))
}

/// A two-page PDF with a two-level outline (Chapter 1 > Section 1.1, Chapter 2)
/// pointing at pages via explicit `/Dest` arrays, plus a page carrying one
/// external-URI link and one internal-destination link, and a full info dict.
/// For outline / link / metadata reading.
pub fn outline_and_links() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page1_id = doc.new_object_id();
    let page2_id = doc.new_object_id();
    let link1_id = doc.new_object_id();
    let link2_id = doc.new_object_id();
    let outlines_id = doc.new_object_id();
    let item1_id = doc.new_object_id();
    let item11_id = doc.new_object_id();
    let item2_id = doc.new_object_id();

    let media_box = || vec![0_i64.into(), 0_i64.into(), PAGE_W.into(), PAGE_H.into()];
    let dest = |page: lopdf::ObjectId, x: i64, y: i64| {
        Object::Array(vec![
            page.into(),
            Object::Name(b"XYZ".to_vec()),
            x.into(),
            y.into(),
            Object::Null,
        ])
    };
    let empty_content = doc.add_object(Stream::new(
        dictionary! {},
        Content { operations: vec![] }
            .encode()
            .expect("encode content stream"),
    ));

    doc.objects.insert(
        page1_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => media_box(),
            "Contents" => empty_content,
            "Annots" => vec![link1_id.into(), link2_id.into()],
        }),
    );
    doc.objects.insert(
        page2_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => media_box(),
            "Contents" => empty_content,
        }),
    );

    doc.objects.insert(
        link1_id,
        Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Link",
            "Rect" => vec![50_i64.into(), 700_i64.into(), 150_i64.into(), 720_i64.into()],
            "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("https://example.com") },
        }),
    );
    doc.objects.insert(
        link2_id,
        Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Link",
            "Rect" => vec![200_i64.into(), 700_i64.into(), 350_i64.into(), 720_i64.into()],
            "Dest" => dest(page2_id, 50, 100),
        }),
    );

    doc.objects.insert(
        item11_id,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("Section 1.1"),
            "Parent" => item1_id,
            "Dest" => dest(page2_id, 100, 200),
        }),
    );
    doc.objects.insert(
        item1_id,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("Chapter 1"),
            "Parent" => outlines_id,
            "Next" => item2_id,
            "First" => item11_id,
            "Last" => item11_id,
            "Count" => 1_i64,
            "Dest" => dest(page1_id, 0, 0),
        }),
    );
    doc.objects.insert(
        item2_id,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("Chapter 2"),
            "Parent" => outlines_id,
            "Prev" => item1_id,
            "Dest" => dest(page2_id, 0, 500),
        }),
    );
    doc.objects.insert(
        outlines_id,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => item1_id,
            "Last" => item2_id,
            "Count" => 2_i64,
        }),
    );
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page1_id.into(), page2_id.into()],
            "Count" => 2_i64,
        }),
    );

    let info_id = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Outline and Link Fixture"),
        "Author" => Object::string_literal("pdfkit"),
        "Subject" => Object::string_literal("outline + link test"),
        "Keywords" => Object::string_literal("outlines, links"),
        "Creator" => Object::string_literal("pdfkit-fixtures"),
        "Producer" => Object::string_literal("lopdf"),
    });
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "Outlines" => outlines_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.trailer.set("Info", info_id);
    let file_id = Object::string_literal(&b"pdfkit-fixture01"[..]);
    doc.trailer
        .set("ID", Object::Array(vec![file_id.clone(), file_id]));
    to_bytes(doc)
}

/// A malformed single-page PDF whose sole outline item points at itself as both
/// `/Next` (sibling) and `/First` (child). Used to prove outline traversal
/// terminates (no hang / stack overflow) on a cyclic outline.
pub fn cyclic_outline() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let outlines_id = doc.new_object_id();
    let item_id = doc.new_object_id();
    let content = doc.add_object(Stream::new(
        dictionary! {},
        Content { operations: vec![] }
            .encode()
            .expect("encode content stream"),
    ));
    doc.objects.insert(
        page_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0_i64.into(), 0_i64.into(), PAGE_W.into(), PAGE_H.into()],
            "Contents" => content,
        }),
    );
    doc.objects.insert(
        item_id,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("Loop"),
            "Parent" => outlines_id,
            "Next" => item_id,  // self-referential sibling
            "First" => item_id, // self-referential child
            "Dest" => Object::Array(vec![
                page_id.into(),
                Object::Name(b"XYZ".to_vec()),
                0_i64.into(),
                0_i64.into(),
                Object::Null,
            ]),
        }),
    );
    doc.objects.insert(
        outlines_id,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => item_id,
            "Last" => item_id,
            "Count" => 1_i64,
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
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "Outlines" => outlines_id,
    });
    doc.trailer.set("Root", catalog_id);
    let file_id = Object::string_literal(&b"pdfkit-fixture01"[..]);
    doc.trailer
        .set("ID", Object::Array(vec![file_id.clone(), file_id]));
    to_bytes(doc)
}

/// A minimal tagged (PDF/UA-style) single-page document: catalog `/MarkInfo
/// /Marked true` + a `/StructTreeRoot` whose `Document` element has `H1`, `P`,
/// and `Figure` (with `/Alt`) children, each pointing at a marked-content MCID
/// in the page stream ("Title" / "Paragraph." / "Figure"). For structure-tree
/// reading.
pub fn tagged_minimal() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let struct_root_id = doc.new_object_id();
    let doc_elem_id = doc.new_object_id();
    let h1_id = doc.new_object_id();
    let p_id = doc.new_object_id();
    let fig_id = doc.new_object_id();

    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });

    // A marked-content sequence drawing `text` at `y` under tag `tag`/`mcid`.
    let marked = |ops: &mut Vec<Operation>, tag: &str, mcid: i64, text: &str, y: f32| {
        ops.push(Operation::new(
            "BDC",
            vec![
                Object::Name(tag.as_bytes().to_vec()),
                Object::Dictionary(dictionary! { "MCID" => mcid }),
            ],
        ));
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
        ops.push(Operation::new("Tj", vec![Object::string_literal(text)]));
        ops.push(Operation::new("EMC", vec![]));
    };
    let mut ops = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F1".into(), 14_i64.into()]),
    ];
    marked(&mut ops, "H1", 0, "Title", 700.0);
    marked(&mut ops, "P", 1, "Paragraph.", 680.0);
    marked(&mut ops, "Figure", 2, "Figure", 660.0);
    ops.push(Operation::new("ET", vec![]));
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        Content { operations: ops }
            .encode()
            .expect("encode content stream"),
    ));

    doc.objects.insert(
        page_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0_i64.into(), 0_i64.into(), PAGE_W.into(), PAGE_H.into()],
            "Contents" => content_id,
            "Resources" => resources_id,
            "StructParents" => 0_i64,
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

    let elem = |s: &str, mcid: i64| {
        dictionary! {
            "Type" => "StructElem",
            "S" => s,
            "P" => doc_elem_id,
            "Pg" => page_id,
            "K" => mcid,
        }
    };
    doc.objects.insert(h1_id, Object::Dictionary(elem("H1", 0)));
    doc.objects.insert(p_id, Object::Dictionary(elem("P", 1)));
    let mut fig = elem("Figure", 2);
    fig.set("Alt", Object::string_literal("A pie chart"));
    doc.objects.insert(fig_id, Object::Dictionary(fig));
    doc.objects.insert(
        doc_elem_id,
        Object::Dictionary(dictionary! {
            "Type" => "StructElem",
            "S" => "Document",
            "P" => struct_root_id,
            "K" => vec![h1_id.into(), p_id.into(), fig_id.into()],
        }),
    );
    doc.objects.insert(
        struct_root_id,
        Object::Dictionary(dictionary! {
            "Type" => "StructTreeRoot",
            "K" => doc_elem_id,
        }),
    );

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "MarkInfo" => dictionary! { "Marked" => true },
        "StructTreeRoot" => struct_root_id,
    });
    doc.trailer.set("Root", catalog_id);
    let info_id = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Tagged Fixture"),
        "Author" => Object::string_literal("pdfkit"),
    });
    doc.trailer.set("Info", info_id);
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

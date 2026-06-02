//! Acceptance test: build a real (minimal) .docx in memory, convert it to PDF,
//! and prove the result is a valid PDF whose text round-trips back out through
//! the pdfkit-core extraction engine.

use std::io::{Cursor, Write};

use pdfkit_core::{extract, ExtractOptions, Mode, PdfError};
use zip::write::SimpleFileOptions;

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Acme Quarterly</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Overview</w:t></w:r></w:p>
    <w:p>
      <w:r><w:t xml:space="preserve">Revenue grew </w:t></w:r>
      <w:r><w:rPr><w:b/></w:rPr><w:t>eighteen</w:t></w:r>
      <w:r><w:t xml:space="preserve"> percent this </w:t></w:r>
      <w:r><w:rPr><w:i/></w:rPr><w:t>quarter</w:t></w:r>
      <w:r><w:t>, a record for the company.</w:t></w:r>
    </w:p>
    <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Alpha highlight</w:t></w:r></w:p>
    <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Beta highlight</w:t></w:r></w:p>
    <w:tbl>
      <w:tr>
        <w:tc><w:p><w:r><w:t>CellOne</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>CellTwo</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>CellThree</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>CellFour</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>
  </w:body>
</w:document>"#;

fn build_docx() -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in [
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", RELS),
            ("word/document.xml", DOCUMENT_XML),
        ] {
            zip.start_file(name, opts).expect("start zip entry");
            zip.write_all(body.as_bytes()).expect("write zip entry");
        }
        zip.finish().expect("finish zip");
    }
    buf.into_inner()
}

#[test]
fn converts_docx_to_a_readable_pdf() {
    let docx = build_docx();
    let pdf = pdfkit_docx::docx_to_pdf(&docx).expect("docx_to_pdf");

    assert!(pdf.starts_with(b"%PDF"), "output should be a PDF");

    let result = extract(
        pdf,
        ExtractOptions {
            mode: Mode::Text,
            ..Default::default()
        },
    )
    .expect("extract text from produced PDF");
    let text = result.text;

    for needle in [
        "Acme", "Overview", "Revenue", "eighteen", "quarter", "Alpha", "Beta", "CellOne",
        "CellFour",
    ] {
        assert!(text.contains(needle), "expected {needle:?} in:\n{text}");
    }
}

/// Build a one-paragraph docx from raw run XML (no styles/numbering).
fn docx_with_runs(body_runs: &str) -> Vec<u8> {
    let doc = format!(
        r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p>{body_runs}</w:p></w:body></w:document>"#
    );
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("word/document.xml", opts).unwrap();
        zip.write_all(doc.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    buf.into_inner()
}

fn extract_text(pdf: Vec<u8>) -> String {
    extract(
        pdf,
        ExtractOptions {
            mode: Mode::Text,
            ..Default::default()
        },
    )
    .expect("extract")
    .text
}

#[test]
fn abutting_runs_keep_no_space_but_separated_runs_do() {
    // Two runs that abut with NO whitespace must read as one word; a run that
    // ends with a space must keep the gap.
    let glued = extract_text(
        pdfkit_docx::docx_to_pdf(&docx_with_runs(
            r#"<w:r><w:t>Pdf</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>Kit</w:t></w:r>"#,
        ))
        .expect("convert"),
    );
    assert!(
        glued.contains("PdfKit"),
        "abutting runs must not gain a space; got {glued:?}"
    );
    assert!(
        !glued.contains("Pdf Kit"),
        "unexpected spurious space; got {glued:?}"
    );

    let spaced = extract_text(
        pdfkit_docx::docx_to_pdf(&docx_with_runs(
            r#"<w:r><w:t xml:space="preserve">Pdf </w:t></w:r><w:r><w:t>Kit</w:t></w:r>"#,
        ))
        .expect("convert"),
    );
    assert!(
        spaced.contains("Pdf Kit"),
        "a real trailing space must be preserved; got {spaced:?}"
    );
}

#[test]
fn rejects_input_that_is_not_a_docx() {
    let err = pdfkit_docx::docx_to_pdf(b"this is plainly not a zip").unwrap_err();
    assert!(matches!(err, PdfError::Format(_)), "got {err:?}");
}

#[test]
fn empty_but_valid_docx_yields_a_pdf() {
    const EMPTY_DOC: &str = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#;
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("word/document.xml", opts).unwrap();
        zip.write_all(EMPTY_DOC.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    let pdf = pdfkit_docx::docx_to_pdf(&buf.into_inner()).expect("convert empty doc");
    assert!(pdf.starts_with(b"%PDF"));
}

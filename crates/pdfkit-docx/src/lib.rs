//! `pdfkit-docx` — convert a Word `.docx` document into a PDF, pure-Rust and
//! offline.
//!
//! The `.docx` (an OPC zip of WordprocessingML) is parsed into a small document
//! model (paragraphs, headings, lists, tables, inline images) and laid out onto
//! PDF pages through the `pdfkit-edit` create path. It is a faithful-enough
//! reading layout, not a full Word rendering engine: single column, US-Letter,
//! standard-14 Helvetica, with bold/italic, headings, bullet/numbered lists,
//! bordered tables, and scaled images.
//!
//! ```no_run
//! let docx: &[u8] = b""; // bytes of a .docx file
//! let pdf: Vec<u8> = pdfkit_docx::docx_to_pdf(docx)?;
//! # Ok::<(), pdfkit_core::PdfError>(())
//! ```

mod layout;
mod metrics;
mod model;
mod parse;

use pdfkit_core::PdfError;

/// Convert the bytes of a `.docx` document to PDF bytes.
///
/// Returns [`PdfError::Format`] if the input is not a readable `.docx`.
pub fn docx_to_pdf(docx: &[u8]) -> Result<Vec<u8>, PdfError> {
    let model = parse::parse_docx(docx)?;
    layout::render(&model)
}

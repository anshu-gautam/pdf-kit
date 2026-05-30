//! `pdfkit-edit` — the write path (create + edit).
//!
//! Depends only on `pdfkit-core` (shares the document model) and never flows
//! through the extraction engine. Exposes [`PdfBuilder`] (author new documents)
//! and [`PdfEditor`] (merge / split / remove / rotate / watermark / fill_form).
//!
//! Implemented in M7 of `Prd.md`.

//! `pdfkit-core` — the foundation crate.
//!
//! Owns the document model, text extraction, page classification, and the
//! [`extract`] entry point. Everything in the workspace depends on this crate.
//!
//! Implemented incrementally per the milestones in `Prd.md`:
//! - M1: `Engine`, `Document`, `Page`, `Metadata`, `PdfError`, text extraction.
//! - M2: page classification.
//! - M4: the `extract` Auto fallback.

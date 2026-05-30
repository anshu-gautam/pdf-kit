//! `pdfkit-render` — page → RGBA → PNG.
//!
//! Backend selected by feature flag: `render-native` (pure-Rust, default) or
//! `render-pdfium` (high-fidelity, native/WASM PDFium). The pixel budget is
//! enforced before any buffer is allocated.
//!
//! Implemented in M3 of `Prd.md`.

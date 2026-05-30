//! Input and option value types for the public API (PRD §4.1).
//!
//! These are deliberately free of any backend types so the public surface never
//! leaks `lopdf` (or any other dependency) to callers.

use std::path::{Path, PathBuf};

/// What to open. The WASM crate adds `Blob`/`ArrayBuffer` conversions on top of
/// `Bytes` at its boundary.
#[derive(Debug, Clone)]
pub enum PdfInput {
    /// Read the document from a filesystem path.
    Path(PathBuf),
    /// Read the document from an in-memory buffer.
    Bytes(Vec<u8>),
}

impl From<PathBuf> for PdfInput {
    fn from(p: PathBuf) -> Self {
        PdfInput::Path(p)
    }
}

impl From<&Path> for PdfInput {
    fn from(p: &Path) -> Self {
        PdfInput::Path(p.to_path_buf())
    }
}

impl From<&PathBuf> for PdfInput {
    fn from(p: &PathBuf) -> Self {
        PdfInput::Path(p.clone())
    }
}

impl From<&str> for PdfInput {
    /// A bare string is treated as a filesystem path.
    fn from(s: &str) -> Self {
        PdfInput::Path(PathBuf::from(s))
    }
}

impl From<String> for PdfInput {
    fn from(s: String) -> Self {
        PdfInput::Path(PathBuf::from(s))
    }
}

impl From<Vec<u8>> for PdfInput {
    fn from(b: Vec<u8>) -> Self {
        PdfInput::Bytes(b)
    }
}

impl From<&[u8]> for PdfInput {
    fn from(b: &[u8]) -> Self {
        PdfInput::Bytes(b.to_vec())
    }
}

/// Options controlling how a document is opened.
#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    /// Password to try if the document is encrypted.
    pub password: Option<String>,
}

impl OpenOptions {
    /// Convenience constructor for an opened-with-password request.
    pub fn with_password(password: impl Into<String>) -> Self {
        OpenOptions {
            password: Some(password.into()),
        }
    }
}

/// Options controlling whole-document text extraction.
#[derive(Debug, Clone)]
pub struct TextOptions {
    /// Restrict extraction to these one-based page numbers (in this order).
    pub pages: Option<Vec<usize>>,
    /// Hard cap on the number of pages visited.
    pub max_pages: usize,
    /// Hard cap on the number of characters returned.
    pub max_chars: usize,
}

impl Default for TextOptions {
    fn default() -> Self {
        TextOptions {
            pages: None,
            max_pages: 20,
            max_chars: 200_000,
        }
    }
}

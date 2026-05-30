//! The single error type for the whole toolkit (PRD §6).
//!
//! Every public function in every crate returns `Result<_, PdfError>`. There
//! are no `unwrap()`/`expect()`/`panic!` calls on any library path.

use std::error::Error as StdError;

/// The one error enum shared across the workspace.
///
/// Backends (lopdf, PDFium, OCR providers) are collapsed into these variants at
/// the crate boundary so callers never depend on a backend's error type.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum PdfError {
    /// Invalid input or a malformed PDF. Carries the underlying parser error
    /// when one is available.
    #[error("invalid input or malformed PDF")]
    Format(#[source] Option<Box<dyn StdError + Send + Sync>>),

    /// A password was required but missing or incorrect.
    #[error("missing or incorrect password")]
    Password,

    /// The document uses a security handler this build does not support.
    #[error("unsupported security handler")]
    Security,

    /// A one-based page number was outside `1..=page_count`.
    #[error("page {0} out of range")]
    PageRange(usize),

    /// A render or extraction budget (pixels, dimensions, chars) was exceeded.
    #[error("render or extraction budget exceeded")]
    Budget,

    /// A handle was used after it was explicitly destroyed (wasm lifecycle).
    #[error("use after destroy")]
    Destroyed,

    /// A backend reported an error that doesn't map to a more specific variant.
    #[error("backend error: {0}")]
    Backend(String),
}

impl PdfError {
    /// Wrap any source error as a [`PdfError::Format`].
    pub fn format<E>(source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        PdfError::Format(Some(Box::new(source)))
    }

    /// A [`PdfError::Format`] with no captured source.
    pub fn malformed() -> Self {
        PdfError::Format(None)
    }
}

/// Collapse a backend `lopdf::Error` into the public error model at the crate
/// boundary, so callers never see (or depend on) the lopdf error type.
impl From<lopdf::Error> for PdfError {
    fn from(e: lopdf::Error) -> Self {
        use lopdf::Error as L;
        match e {
            // A missing or wrong user password both surface here (the reader
            // tries the empty password first, then the supplied one).
            L::InvalidPassword => PdfError::Password,
            L::UnsupportedSecurityHandler(_) => PdfError::Security,
            L::Decryption(_) => PdfError::Security,
            L::PageNumberNotFound(n) => PdfError::PageRange(n as usize),
            // Everything else is a malformed/unreadable document; keep the
            // backend error as the source for diagnostics.
            other => PdfError::format(other),
        }
    }
}

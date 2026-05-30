//! `pdfkit-render` — page → RGBA → PNG.
//!
//! The render value types, the [`Renderer`] trait, the pure-Rust
//! [`NativeRenderer`], and [`encode_png`] are defined in `pdfkit-core` so the
//! in-core `extract` entry point can use them without a dependency cycle
//! (see the M3 commit notes). This crate is the public render surface: it
//! re-exports them and adds the high-fidelity PDFIUM backend behind
//! `render-pdfium`.
//!
//! Backend selection:
//! - `render-native` (default): [`NativeRenderer`], best-effort pure Rust.
//! - `render-pdfium`: [`PdfiumRenderer`], high fidelity (binds PDFIUM at
//!   runtime).

pub use pdfkit_core::{Background, Bitmap, RenderOptions, Renderer};

#[cfg(feature = "render-native")]
pub use pdfkit_core::{encode_png, NativeRenderer};

#[cfg(feature = "render-pdfium")]
pub use pdfium::PdfiumRenderer;

#[cfg(feature = "render-pdfium")]
mod pdfium {
    use pdfkit_core::{Bitmap, Page, PdfError, RenderOptions, Renderer};

    /// High-fidelity renderer backed by PDFIUM.
    ///
    // TODO(design): wire the `pdfium-render` crate (PRD §4.2 / §12.4). PDFIUM
    // binds to a native (or WASM) library at runtime; that library is not
    // available in the current build environment, so this is a typed
    // placeholder that keeps the feature and API stable. The default
    // `render-native` path is fully implemented and tested.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct PdfiumRenderer;

    impl Renderer for PdfiumRenderer {
        fn render(&self, _page: &Page, _opts: &RenderOptions) -> Result<Bitmap, PdfError> {
            Err(PdfError::Backend(
                "pdfium backend not yet wired; build with the default render-native feature".into(),
            ))
        }
    }
}

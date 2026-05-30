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
    use std::path::Path;

    use pdfium_render::prelude::*;

    use pdfkit_core::{Bitmap, PdfError, RenderOptions};

    /// High-fidelity renderer backed by PDFIUM (Google's PDF engine).
    ///
    /// PDFIUM has its own parser, so it renders from the original document bytes
    /// (and page index) rather than from a `pdfkit_core::Page` (which is the
    /// lopdf-based model). Use [`PdfiumRenderer::render_page`].
    ///
    /// The PDFIUM library is located at runtime, in order:
    /// 1. `$PDFKIT_PDFIUM_LIB` (full path to the library file),
    /// 2. `~/.cache/pdfkit/pdfium/lib/<platform name>` (see scripts/fetch-pdfium.sh),
    /// 3. the system library search path.
    pub struct PdfiumRenderer {
        pdfium: Pdfium,
    }

    impl PdfiumRenderer {
        /// Bind to the PDFIUM library and create a renderer.
        pub fn new() -> Result<Self, PdfError> {
            let bindings =
                load_bindings().map_err(|e| PdfError::Backend(format!("pdfium: {e}")))?;
            Ok(PdfiumRenderer {
                pdfium: Pdfium::new(bindings),
            })
        }

        /// Render a one-based page of `pdf` to a [`Bitmap`], honoring the sizing
        /// and pixel budget in `opts`. Unlike the native path, this rasterizes
        /// text and vector content.
        pub fn render_page(
            &self,
            pdf: &[u8],
            page_one_based: usize,
            password: Option<&str>,
            opts: &RenderOptions,
        ) -> Result<Bitmap, PdfError> {
            let document = self
                .pdfium
                .load_pdf_from_byte_slice(pdf, password)
                .map_err(map_err)?;
            let pages = document.pages();
            let count = pages.len() as usize;
            if page_one_based == 0 || page_one_based > count {
                return Err(PdfError::PageRange(page_one_based));
            }
            let page = pages
                .get((page_one_based - 1) as PdfPageIndex)
                .map_err(map_err)?;

            let (width, height) = output_dimensions(page.width().value, page.height().value, opts)?;
            let config = PdfRenderConfig::new()
                .set_target_size(width as i32, height as i32)
                .render_form_data(opts.forms);
            let bitmap = page.render_with_config(&config).map_err(map_err)?;

            Ok(Bitmap {
                width: bitmap.width() as u32,
                height: bitmap.height() as u32,
                rgba: bitmap.as_rgba_bytes(),
            })
        }
    }

    fn load_bindings() -> Result<Box<dyn PdfiumLibraryBindings>, PdfiumError> {
        if let Some(path) = std::env::var_os("PDFKIT_PDFIUM_LIB") {
            return Pdfium::bind_to_library(path);
        }
        if let Some(home) = std::env::var_os("HOME") {
            let folder = Path::new(&home)
                .join(".cache")
                .join("pdfkit")
                .join("pdfium")
                .join("lib");
            let lib = Pdfium::pdfium_platform_library_name_at_path(&folder);
            if lib.exists() {
                return Pdfium::bind_to_library(lib);
            }
        }
        Pdfium::bind_to_system_library()
    }

    fn map_err(error: PdfiumError) -> PdfError {
        match error {
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                PdfError::Password
            }
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::SecurityError) => {
                PdfError::Security
            }
            other => PdfError::Backend(format!("pdfium: {other}")),
        }
    }

    /// Compute output `(width, height)` from the page size in points and the
    /// render options, enforcing the pixel/dimension budget. Mirrors the native
    /// renderer's sizing (that helper is gated behind `render-native`).
    fn output_dimensions(
        page_w_pt: f32,
        page_h_pt: f32,
        opts: &RenderOptions,
    ) -> Result<(u32, u32), PdfError> {
        let pw = page_w_pt.max(1.0);
        let ph = page_h_pt.max(1.0);
        let (w, h) = match (opts.width, opts.height) {
            (Some(w), Some(h)) => (w as f32, h as f32),
            (Some(w), None) => (w as f32, w as f32 * ph / pw),
            (None, Some(h)) => (h as f32 * pw / ph, h as f32),
            (None, None) => {
                let scale = opts
                    .dpi
                    .map(|d| d / 72.0)
                    .or(opts.scale)
                    .unwrap_or(150.0 / 72.0);
                (pw * scale, ph * scale)
            }
        };
        let width = (w.round() as u32).max(1);
        let height = (h.round() as u32).max(1);
        if width > opts.max_dimension || height > opts.max_dimension {
            return Err(PdfError::Budget);
        }
        if u64::from(width) * u64::from(height) > u64::from(opts.max_pixels) {
            return Err(PdfError::Budget);
        }
        Ok((width, height))
    }
}

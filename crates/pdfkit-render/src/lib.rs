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
    use std::sync::{Mutex, OnceLock};

    use pdfium_render::prelude::*;

    use pdfkit_core::{Bitmap, PdfError, RenderOptions};

    // PDFIUM binds its library into a process-global once (pdfium-render keeps a
    // global OnceCell, and `Pdfium::new` asserts it is unset). So we hold a single
    // shared `Pdfium` for the process: any number of PdfiumRenderers reuse it, a
    // second construction no longer errors, and the init is serialized so racing
    // threads can't both reach `Pdfium::new` and panic.
    static PDFIUM: OnceLock<Pdfium> = OnceLock::new();
    static PDFIUM_INIT: Mutex<()> = Mutex::new(());

    fn shared_pdfium() -> Result<&'static Pdfium, PdfError> {
        if let Some(pdfium) = PDFIUM.get() {
            return Ok(pdfium);
        }
        let _guard = PDFIUM_INIT
            .lock()
            .map_err(|_| PdfError::Backend("pdfium init lock poisoned".into()))?;
        if let Some(pdfium) = PDFIUM.get() {
            return Ok(pdfium);
        }
        let bindings = load_bindings().map_err(|e| PdfError::Backend(format!("pdfium: {e}")))?;
        let _ = PDFIUM.set(Pdfium::new(bindings));
        PDFIUM
            .get()
            .ok_or_else(|| PdfError::Backend("pdfium initialization failed".into()))
    }

    /// High-fidelity renderer backed by PDFIUM (Google's PDF engine).
    ///
    /// PDFIUM has its own parser, so it renders from the original document bytes
    /// (and page index) rather than from a `pdfkit_core::Page` (which is the
    /// lopdf-based model). Use [`PdfiumRenderer::render_page`]. All instances
    /// share one process-wide PDFIUM binding.
    ///
    /// The PDFIUM library is located at runtime, in order:
    /// 1. `$PDFKIT_PDFIUM_LIB` (full path to the library file),
    /// 2. `~/.cache/pdfkit/pdfium/lib/<platform name>` (see scripts/fetch-pdfium.sh),
    /// 3. the system library search path.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct PdfiumRenderer {
        _private: (),
    }

    impl PdfiumRenderer {
        /// Bind to (or reuse) the PDFIUM library and create a renderer handle.
        pub fn new() -> Result<Self, PdfError> {
            shared_pdfium()?; // surface a binding failure eagerly
            Ok(PdfiumRenderer { _private: () })
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
            let document = shared_pdfium()?
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

            let (width, height) =
                opts.output_dimensions(page.width().value, page.height().value)?;
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
}

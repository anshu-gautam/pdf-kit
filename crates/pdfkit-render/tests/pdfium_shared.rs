//! #2 regression: multiple PdfiumRenderer instances share one process-wide
//! PDFIUM binding, so a second construction no longer errors. Requires the
//! PDFIUM library; run with:
//!
//!   cargo test -p pdfkit-render --features render-pdfium --test pdfium_shared -- --ignored
#![cfg(feature = "render-pdfium")]

use pdfkit_render::{PdfiumRenderer, RenderOptions};

#[test]
#[ignore = "requires the PDFIUM library"]
fn two_renderers_coexist() {
    let a = PdfiumRenderer::new().expect("first renderer");
    // Before the fix this errored with PdfiumLibraryBindingsAlreadyInitialized.
    let b = PdfiumRenderer::new().expect("second renderer");

    let pdf = pdfkit_fixtures::born_digital();
    let opts = RenderOptions {
        width: Some(80),
        ..Default::default()
    };
    assert!(a.render_page(&pdf, 1, None, &opts).is_ok());
    assert!(b.render_page(&pdf, 1, None, &opts).is_ok());
}

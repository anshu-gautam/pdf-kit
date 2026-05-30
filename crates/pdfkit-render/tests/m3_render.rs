//! M3 acceptance: render a page to a valid PNG of the expected dimensions, and
//! reject oversized requests with PdfError::Budget.

use pdfkit_core::{Engine, OpenOptions, PdfError};
use pdfkit_render::{encode_png, Bitmap, NativeRenderer, RenderOptions, Renderer};

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

fn render_first(bytes: Vec<u8>, opts: RenderOptions) -> Result<Bitmap, PdfError> {
    let doc = Engine::new()
        .expect("engine")
        .open(bytes, OpenOptions::default())
        .expect("open");
    let page = doc.page(1).expect("page 1");
    NativeRenderer.render(&page, &opts)
}

fn center_pixel(bmp: &Bitmap) -> [u8; 4] {
    let x = bmp.width / 2;
    let y = bmp.height / 2;
    let off = ((y * bmp.width + x) * 4) as usize;
    [
        bmp.rgba[off],
        bmp.rgba[off + 1],
        bmp.rgba[off + 2],
        bmp.rgba[off + 3],
    ]
}

#[test]
fn renders_scanned_page_to_expected_dimensions_and_png() {
    let opts = RenderOptions {
        width: Some(120),
        ..Default::default()
    };
    let bmp = render_first(pdfkit_fixtures::scanned(), opts).expect("render");

    // 612 x 792 page, width pinned to 120 -> height = round(120 * 792/612).
    assert_eq!(bmp.width, 120);
    assert_eq!(bmp.height, 155);
    assert_eq!(bmp.rgba.len(), 120 * 155 * 4);

    // The full-page gray image (value 160) should be composited at the center.
    let [r, g, b, a] = center_pixel(&bmp);
    assert_eq!(
        (r, g, b, a),
        (160, 160, 160, 255),
        "expected composited gray"
    );

    let png = encode_png(&bmp, false).expect("encode png");
    assert!(
        png.starts_with(&PNG_SIGNATURE),
        "not a PNG: {:?}",
        &png[..8.min(png.len())]
    );
    assert!(png.len() > 50);
}

#[test]
fn renders_text_page_as_blank_background() {
    let opts = RenderOptions {
        width: Some(100),
        ..Default::default()
    };
    let bmp = render_first(pdfkit_fixtures::born_digital(), opts).expect("render");
    assert_eq!(bmp.width, 100);
    // No images -> white background (native path does not rasterize text).
    assert_eq!(center_pixel(&bmp), [255, 255, 255, 255]);

    let png = encode_png(&bmp, true).expect("encode png");
    assert!(png.starts_with(&PNG_SIGNATURE));
}

#[test]
fn oversized_request_hits_budget() {
    // 612pt at 20000 DPI is ~170k px wide -> exceeds max_dimension (10_000).
    let opts = RenderOptions {
        dpi: Some(20_000.0),
        ..Default::default()
    };
    let err = render_first(pdfkit_fixtures::born_digital(), opts).expect_err("must exceed budget");
    assert!(matches!(err, PdfError::Budget), "got {err:?}");
}

#[test]
fn pixel_budget_enforced_independently_of_dimension() {
    // Dimensions individually under max_dimension, but total pixels over budget.
    let opts = RenderOptions {
        width: Some(3000),
        height: Some(3000),
        max_pixels: 1_000_000,
        ..Default::default()
    };
    let err = render_first(pdfkit_fixtures::born_digital(), opts).expect_err("must exceed budget");
    assert!(matches!(err, PdfError::Budget), "got {err:?}");
}

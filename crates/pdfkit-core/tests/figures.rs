//! Figure / image-region detection, caption pairing, and region cropping.

use pdfkit_core::{Document, Engine, OpenOptions};

fn open(bytes: Vec<u8>) -> Document {
    Engine::new()
        .unwrap()
        .open(bytes, OpenOptions::default())
        .expect("open")
}

#[test]
fn image_region_bbox_and_caption() {
    let doc = open(pdfkit_fixtures::figure_with_caption());
    let regions = doc.page(1).expect("page 1").image_regions();
    assert_eq!(regions.len(), 1, "one embedded image");

    let r = &regions[0];
    let near = |a: f32, b: f32| (a - b).abs() < 1.0;
    assert!(
        near(r.bbox[0], 100.0)
            && near(r.bbox[1], 400.0)
            && near(r.bbox[2], 500.0)
            && near(r.bbox[3], 700.0),
        "bbox {:?}",
        r.bbox
    );
    assert_eq!(r.caption.as_deref(), Some("Figure 1: A sample chart."));
}

#[test]
fn pages_without_images_have_no_regions() {
    let doc = open(pdfkit_fixtures::born_digital());
    assert!(doc.page(1).expect("page 1").image_regions().is_empty());
}

#[test]
fn crop_rejects_a_malformed_bitmap_without_panicking() {
    use pdfkit_core::Bitmap;
    // Dimensions claim a huge buffer the rgba doesn't back (and would overflow
    // naive u32 index math): must return 0x0, not panic or read out of bounds.
    let bitmap = Bitmap {
        width: 1_000_000_000,
        height: 4,
        rgba: vec![0u8; 16],
    };
    let out = bitmap.crop(0, 1, 10, 10);
    assert_eq!((out.width, out.height), (0, 0));
}

#[cfg(feature = "render-native")]
#[test]
fn crop_region_extracts_a_subimage() {
    use pdfkit_core::{encode_png, NativeRenderer, RenderOptions, Renderer};

    let doc = open(pdfkit_fixtures::figure_with_caption());
    let page = doc.page(1).expect("page 1");
    let (pw, ph) = page.size_points();
    let bitmap = NativeRenderer
        .render(&page, &RenderOptions::default())
        .expect("render");
    let region = page.image_regions()[0].bbox;

    let crop = bitmap.crop_region(pw, ph, region);
    assert!(crop.width > 0 && crop.height > 0);
    assert!(
        crop.width < bitmap.width && crop.height < bitmap.height,
        "the figure region is a strict sub-rectangle of the page"
    );
    let png = encode_png(&crop, true).expect("encode");
    assert!(png.starts_with(&[137, 80, 78, 71]), "valid PNG");
}

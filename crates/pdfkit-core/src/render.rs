//! Page rendering: page → RGBA → PNG (PRD §4.2).
//!
//! The value types and the [`Renderer`] trait are always available. The
//! pure-Rust [`NativeRenderer`] and [`encode_png`] are compiled only with the
//! `render-native` feature (which pulls in the `image` crate). The PDFIUM
//! backend lives in `pdfkit-render` behind `render-pdfium`.
//!
//! The native path is best-effort (PRD §4.2): it sizes the page correctly,
//! fills the background, and composites embedded raster images (the scanned-page
//! case). It does not rasterize vector/text content.

use crate::document::Page;
use crate::error::PdfError;

/// Page background to paint before drawing content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Background {
    /// Opaque white.
    #[default]
    White,
    /// Fully transparent.
    Transparent,
}

/// A rendered page as a tightly-packed RGBA8 buffer.
#[derive(Clone, PartialEq, Eq)]
pub struct Bitmap {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes, row-major, RGBA8.
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bitmap")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("rgba_len", &self.rgba.len())
            .finish()
    }
}

/// How to size and paint a rendered page.
///
/// Sizing precedence: explicit `width`/`height` win, else `dpi`, else `scale`,
/// else a default of 150 DPI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderOptions {
    /// Target resolution in dots per inch.
    pub dpi: Option<f32>,
    /// Uniform scale relative to the 72-DPI page size.
    pub scale: Option<f32>,
    /// Target width in pixels (height derived if absent).
    pub width: Option<u32>,
    /// Target height in pixels (width derived if absent).
    pub height: Option<u32>,
    /// Background to paint.
    pub background: Background,
    /// Render AcroForm widgets (PDFIUM backend only; ignored by native).
    pub forms: bool,
    /// Maximum output pixels; checked before allocation.
    pub max_pixels: u32,
    /// Maximum output dimension on either axis; checked before allocation.
    pub max_dimension: u32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            dpi: None,
            scale: None,
            width: None,
            height: None,
            background: Background::White,
            forms: false,
            max_pixels: 4_000_000,
            max_dimension: 10_000,
        }
    }
}

/// Default rendering resolution when neither dpi/scale/size is given.
#[cfg(feature = "render-native")]
const DEFAULT_DPI: f32 = 150.0;

#[cfg(feature = "render-native")]
impl RenderOptions {
    /// Compute the integer output `(width, height)` for a page of the given
    /// size in points, enforcing the pixel budget *before* any allocation.
    pub(crate) fn output_dimensions(
        &self,
        page_w_pt: f32,
        page_h_pt: f32,
    ) -> Result<(u32, u32), PdfError> {
        let pw = page_w_pt.max(1.0);
        let ph = page_h_pt.max(1.0);

        let (w, h) = match (self.width, self.height) {
            (Some(w), Some(h)) => (w as f32, h as f32),
            (Some(w), None) => (w as f32, w as f32 * ph / pw),
            (None, Some(h)) => (h as f32 * pw / ph, h as f32),
            (None, None) => {
                let scale = self
                    .dpi
                    .map(|d| d / 72.0)
                    .or(self.scale)
                    .unwrap_or(DEFAULT_DPI / 72.0);
                (pw * scale, ph * scale)
            }
        };

        let width = (w.round() as u32).max(1);
        let height = (h.round() as u32).max(1);

        if width > self.max_dimension || height > self.max_dimension {
            return Err(PdfError::Budget);
        }
        if u64::from(width) * u64::from(height) > u64::from(self.max_pixels) {
            return Err(PdfError::Budget);
        }
        Ok((width, height))
    }
}

/// Render a page to a [`Bitmap`]. Implemented by the native and PDFIUM backends.
pub trait Renderer {
    /// Render `page` to pixels per `opts`.
    fn render(&self, page: &Page, opts: &RenderOptions) -> Result<Bitmap, PdfError>;
}

#[cfg(feature = "render-native")]
pub use native::{encode_png, NativeRenderer};

#[cfg(feature = "render-native")]
mod native {
    use super::{Background, Bitmap, RenderOptions, Renderer};
    use crate::classify;
    use crate::document::Page;
    use crate::error::PdfError;
    use lopdf::{Document as LoDoc, Object, ObjectId};

    /// Pure-Rust, best-effort renderer (PRD §4.2). Sizes the page, paints the
    /// background, and composites axis-aligned embedded raster images.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct NativeRenderer;

    impl Renderer for NativeRenderer {
        fn render(&self, page: &Page, opts: &RenderOptions) -> Result<Bitmap, PdfError> {
            let (pw, ph) = page.size_points();
            let (out_w, out_h) = opts.output_dimensions(pw, ph)?; // budget checked here

            let fill = match opts.background {
                Background::White => [255u8, 255, 255, 255],
                Background::Transparent => [0u8, 0, 0, 0],
            };
            let mut rgba = vec![0u8; (out_w as usize) * (out_h as usize) * 4];
            for px in rgba.chunks_exact_mut(4) {
                px.copy_from_slice(&fill);
            }

            let (doc, page_id) = page.render_handle();
            let scale_x = out_w as f32 / pw.max(1.0);
            let scale_y = out_h as f32 / ph.max(1.0);

            for draw in classify::image_draws(doc, page_id) {
                composite_image(&mut rgba, out_w, out_h, scale_x, scale_y, doc, draw);
            }

            Ok(Bitmap {
                width: out_w,
                height: out_h,
                rgba,
            })
        }
    }

    /// Blit one image draw into the output buffer (axis-aligned placements only;
    /// rotated/skewed placements are skipped as best-effort).
    fn composite_image(
        rgba: &mut [u8],
        out_w: u32,
        out_h: u32,
        scale_x: f32,
        scale_y: f32,
        doc: &LoDoc,
        draw: classify::ImageDraw,
    ) {
        let [a, b, c, d, e, f] = draw.ctm;
        if b.abs() > 1e-3 || c.abs() > 1e-3 {
            return; // not axis-aligned
        }
        let Some(src) = decode_image(doc, draw.id) else {
            return;
        };
        let (iw, ih) = (src.width(), src.height());
        if iw == 0 || ih == 0 {
            return;
        }

        // User-space rect of the placement, normalized so x0<x1, y0<y1.
        let (ux0, ux1) = order(e, e + a);
        let (uy0, uy1) = order(f, f + d);

        // Device rect (flip Y: PDF origin is bottom-left, device is top-left).
        let dx0 = (ux0 * scale_x).floor().max(0.0) as u32;
        let dx1 = (ux1 * scale_x).ceil().min(out_w as f32) as u32;
        let dy0 = ((out_h as f32) - uy1 * scale_y).floor().max(0.0) as u32;
        let dy1 = ((out_h as f32) - uy0 * scale_y).ceil().min(out_h as f32) as u32;
        if dx1 <= dx0 || dy1 <= dy0 {
            return;
        }
        let span_x = (dx1 - dx0) as f32;
        let span_y = (dy1 - dy0) as f32;

        for dy in dy0..dy1 {
            // v=0 at the top of the device rect maps to image row 0.
            let v = (dy - dy0) as f32 / span_y;
            let sy = ((v * ih as f32) as u32).min(ih - 1);
            for dx in dx0..dx1 {
                let u = (dx - dx0) as f32 / span_x;
                let sx = ((u * iw as f32) as u32).min(iw - 1);
                let p = src.get_pixel(sx, sy).0;
                let off = ((dy as usize) * (out_w as usize) + dx as usize) * 4;
                rgba[off..off + 4].copy_from_slice(&p);
            }
        }
    }

    fn order(p: f32, q: f32) -> (f32, f32) {
        if p <= q {
            (p, q)
        } else {
            (q, p)
        }
    }

    /// Decode an image XObject to RGBA8. Supports DCTDecode (JPEG) and raw
    /// DeviceGray/DeviceRGB samples (FlateDecode or none). Returns None for
    /// unsupported encodings (best-effort).
    fn decode_image(doc: &LoDoc, id: ObjectId) -> Option<image::RgbaImage> {
        let stream = doc.get_object(id).and_then(Object::as_stream).ok()?;
        let dict = &stream.dict;
        let w = dict.get(b"Width").and_then(Object::as_i64).ok()? as u32;
        let h = dict.get(b"Height").and_then(Object::as_i64).ok()? as u32;
        if w == 0 || h == 0 {
            return None;
        }

        let filters = filter_names(dict.get(b"Filter").ok());
        if filters.iter().any(|f| f == b"DCTDecode") {
            let img =
                image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg)
                    .ok()?;
            return Some(img.to_rgba8());
        }

        // Raw samples: inflate FlateDecode if present, else use content as-is.
        let data = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        let color_space = dict.get(b"ColorSpace").and_then(Object::as_name).ok();
        let bpc = dict
            .get(b"BitsPerComponent")
            .and_then(Object::as_i64)
            .unwrap_or(8);
        if bpc != 8 {
            return None;
        }

        let mut out = image::RgbaImage::new(w, h);
        match color_space {
            Some(cs) if cs == b"DeviceGray" => {
                let expected = (w as usize) * (h as usize);
                if data.len() < expected {
                    return None;
                }
                for (i, px) in out.pixels_mut().enumerate() {
                    let g = data[i];
                    *px = image::Rgba([g, g, g, 255]);
                }
            }
            Some(cs) if cs == b"DeviceRGB" => {
                let expected = (w as usize) * (h as usize) * 3;
                if data.len() < expected {
                    return None;
                }
                for (i, px) in out.pixels_mut().enumerate() {
                    let o = i * 3;
                    *px = image::Rgba([data[o], data[o + 1], data[o + 2], 255]);
                }
            }
            _ => return None,
        }
        Some(out)
    }

    /// Collect filter names from a `/Filter` entry (a single name or an array).
    fn filter_names(obj: Option<&Object>) -> Vec<Vec<u8>> {
        match obj {
            Some(Object::Name(n)) => vec![n.clone()],
            Some(Object::Array(items)) => items
                .iter()
                .filter_map(|o| o.as_name().ok().map(<[u8]>::to_vec))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Encode a [`Bitmap`] as PNG. `compress` trades speed for size.
    pub fn encode_png(bmp: &Bitmap, compress: bool) -> Result<Vec<u8>, PdfError> {
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        use image::{ExtendedColorType, ImageEncoder};

        let expected = (bmp.width as usize) * (bmp.height as usize) * 4;
        if bmp.rgba.len() != expected {
            return Err(PdfError::Backend("bitmap buffer size mismatch".into()));
        }
        let mut out = Vec::new();
        let compression = if compress {
            CompressionType::Best
        } else {
            CompressionType::Fast
        };
        PngEncoder::new_with_quality(&mut out, compression, FilterType::Adaptive)
            .write_image(&bmp.rgba, bmp.width, bmp.height, ExtendedColorType::Rgba8)
            .map_err(|e| PdfError::Backend(format!("png encode: {e}")))?;
        Ok(out)
    }
}

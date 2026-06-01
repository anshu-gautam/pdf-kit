//! HTTP glue: parse multipart, validate, run the (blocking) service on a worker
//! thread, and map results/errors to responses (PRD §13.5, §13.6).

use std::io::{Cursor, Write};

use axum::extract::Multipart;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::dto::*;
use crate::error::ApiErr;
use crate::service::{self, ChunkOutput};

/// A collected multipart upload: file parts (`file` / `files`) and an optional
/// JSON `options` part.
struct Upload {
    files: Vec<Vec<u8>>,
    options: Option<String>,
}

async fn collect(mut mp: Multipart) -> Result<Upload, ApiErr> {
    let mut files = Vec::new();
    let mut options = None;
    while let Some(field) = mp
        .next_field()
        .await
        .map_err(|e| ApiErr::bad_request("bad_multipart", e.to_string()))?
    {
        match field.name() {
            Some("file") | Some("files") => {
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| ApiErr::bad_request("bad_multipart", e.to_string()))?;
                files.push(data.to_vec());
            }
            Some("options") => {
                options = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiErr::bad_request("bad_multipart", e.to_string()))?,
                );
            }
            // Drain unknown parts so the stream advances.
            _ => {
                let _ = field.bytes().await;
            }
        }
    }
    Ok(Upload { files, options })
}

impl Upload {
    /// Exactly one PDF file part, validated.
    fn one_file(&mut self) -> Result<Vec<u8>, ApiErr> {
        if self.files.len() != 1 {
            return Err(ApiErr::bad_request(
                "expected_one_file",
                format!("expected exactly one `file` part, got {}", self.files.len()),
            ));
        }
        let bytes = self.files.remove(0);
        validate_pdf(&bytes)?;
        Ok(bytes)
    }

    /// Parse the optional `options` JSON, falling back to the default.
    fn parse_options<T: serde::de::DeserializeOwned + Default>(&self) -> Result<T, ApiErr> {
        match self.options.as_deref() {
            None => Ok(T::default()),
            Some(s) if s.trim().is_empty() => Ok(T::default()),
            Some(s) => serde_json::from_str(s)
                .map_err(|e| ApiErr::bad_request("bad_options", e.to_string())),
        }
    }

    /// Parse a required `options` JSON (endpoints with no sensible default).
    fn require_options<T: serde::de::DeserializeOwned>(&self) -> Result<T, ApiErr> {
        match self.options.as_deref() {
            Some(s) if !s.trim().is_empty() => serde_json::from_str(s)
                .map_err(|e| ApiErr::bad_request("bad_options", e.to_string())),
            _ => Err(ApiErr::bad_request(
                "missing_options",
                "this endpoint requires an `options` JSON part",
            )),
        }
    }
}

/// Reject obvious non-PDFs early: the spec permits leading bytes, so scan the
/// first 1024 bytes for `%PDF-` rather than requiring it at offset 0 (PRD §13.5).
fn validate_pdf(bytes: &[u8]) -> Result<(), ApiErr> {
    let window = &bytes[..bytes.len().min(1024)];
    if window.windows(5).any(|w| w == b"%PDF-") {
        Ok(())
    } else {
        Err(ApiErr::bad_request(
            "not_a_pdf",
            "input does not look like a PDF (no %PDF- header in the first 1024 bytes)",
        ))
    }
}

/// Run CPU-bound library work on a blocking worker so the async runtime isn't
/// starved (PRD §13.5).
async fn blocking<T, F>(f: F) -> Result<T, ApiErr>
where
    F: FnOnce() -> Result<T, pdfkit_core::PdfError> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(result) => result.map_err(ApiErr::from),
        Err(e) => Err(ApiErr::internal(format!("worker join error: {e}"))),
    }
}

fn pdf_response(pdf: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, "application/pdf")], pdf).into_response()
}

fn zip_parts(parts: &[Vec<u8>]) -> Result<Vec<u8>, ApiErr> {
    use zip::write::SimpleFileOptions;
    let mut buf = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buf);
        let opts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (i, part) in parts.iter().enumerate() {
            writer
                .start_file(format!("part-{:03}.pdf", i + 1), opts)
                .map_err(|e| ApiErr::internal(e.to_string()))?;
            writer
                .write_all(part)
                .map_err(|e| ApiErr::internal(e.to_string()))?;
        }
        writer
            .finish()
            .map_err(|e| ApiErr::internal(e.to_string()))?;
    }
    Ok(buf.into_inner())
}

// ---------------------------------------------------------------------------
// Read path
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/v1/extract",
    tag = "read",
    request_body(content_type = "multipart/form-data", description = "Parts: `file` (PDF), `options` (JSON ExtractRequest)"),
    responses(
        (status = 200, body = ExtractResponse, description = "Extracted text + rendered page images (base64)"),
        (status = 400, body = ApiError, description = "Malformed or non-PDF input"),
        (status = 422, body = ApiError, description = "Password required/incorrect or page out of range")
    )
)]
pub async fn extract(mp: Multipart) -> Result<Json<ExtractResponse>, ApiErr> {
    let mut up = collect(mp).await?;
    let bytes = up.one_file()?;
    let req: ExtractRequest = up.parse_options()?;
    let resp = blocking(move || service::run_extract(bytes, &req)).await?;
    Ok(Json(resp))
}

#[utoipa::path(
    post,
    path = "/v1/metadata",
    tag = "read",
    request_body(content_type = "multipart/form-data", description = "Parts: `file` (PDF), `options` (JSON `{ password? }`)"),
    responses(
        (status = 200, body = MetadataResponse, description = "Metadata, outline, and per-page links"),
        (status = 400, body = ApiError, description = "Malformed or non-PDF input")
    )
)]
pub async fn metadata(mp: Multipart) -> Result<Json<MetadataResponse>, ApiErr> {
    let mut up = collect(mp).await?;
    let bytes = up.one_file()?;
    let req: MetadataRequest = up.parse_options()?;
    let resp = blocking(move || service::run_metadata(bytes, req.password)).await?;
    Ok(Json(resp))
}

#[utoipa::path(
    post,
    path = "/v1/chunks",
    tag = "read",
    request_body(content_type = "multipart/form-data", description = "Parts: `file` (PDF), `options` (JSON ChunkRequest; `format` = json|markdown)"),
    responses(
        (status = 200, description = "Chunks as JSON `{chunks, document_text}` or Markdown"),
        (status = 400, body = ApiError, description = "Malformed or non-PDF input")
    )
)]
pub async fn chunks(mp: Multipart) -> Result<Response, ApiErr> {
    let mut up = collect(mp).await?;
    let bytes = up.one_file()?;
    let req: ChunkRequest = up.parse_options()?;
    let out = blocking(move || service::run_chunks(bytes, &req)).await?;
    Ok(match out {
        ChunkOutput::Json(v) => Json(v).into_response(),
        ChunkOutput::Markdown(s) => {
            ([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], s).into_response()
        }
    })
}

#[utoipa::path(
    post,
    path = "/v1/figures",
    tag = "read",
    request_body(content_type = "multipart/form-data", description = "Parts: `file` (PDF)"),
    responses((status = 200, body = FiguresResponse, description = "Detected figures + captions per page"))
)]
pub async fn figures(mp: Multipart) -> Result<Json<FiguresResponse>, ApiErr> {
    let mut up = collect(mp).await?;
    let bytes = up.one_file()?;
    let req: FiguresRequest = up.parse_options()?;
    let resp = blocking(move || service::run_figures(bytes, req.password)).await?;
    Ok(Json(resp))
}

#[cfg(feature = "render-pdfium")]
#[utoipa::path(
    post,
    path = "/v1/render",
    tag = "read",
    request_body(content_type = "multipart/form-data", description = "Parts: `file` (PDF), `options` (JSON RenderRequest; one-based `page`, dpi/scale/width/height, background)"),
    responses(
        (status = 200, content_type = "image/png", description = "Rendered page as PNG"),
        (status = 422, body = ApiError, description = "Page out of range or pixel budget exceeded")
    )
)]
pub async fn render(mp: Multipart) -> Result<Response, ApiErr> {
    let mut up = collect(mp).await?;
    let bytes = up.one_file()?;
    let req: RenderRequest = up.parse_options()?;
    let png = blocking(move || service::run_render(bytes, &req.params, req.password)).await?;
    Ok(([(header::CONTENT_TYPE, "image/png")], png).into_response())
}

#[cfg(not(feature = "render-pdfium"))]
#[utoipa::path(
    post,
    path = "/v1/render",
    tag = "read",
    responses((status = 501, body = ApiError, description = "This build has no render-pdfium feature"))
)]
pub async fn render() -> Result<Response, ApiErr> {
    Err(ApiErr::not_implemented(
        "rendering requires a build with --features render-pdfium (PRD §13.2)",
    ))
}

// ---------------------------------------------------------------------------
// Write path
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/v1/edit/merge",
    tag = "edit",
    request_body(content_type = "multipart/form-data", description = "Two or more `files` (PDF) parts, merged in order"),
    responses(
        (status = 200, content_type = "application/pdf", description = "Merged PDF"),
        (status = 400, body = ApiError, description = "Fewer than two files or non-PDF input")
    )
)]
pub async fn edit_merge(mp: Multipart) -> Result<Response, ApiErr> {
    let up = collect(mp).await?;
    if up.files.len() < 2 {
        return Err(ApiErr::bad_request(
            "expected_multiple_files",
            format!(
                "merge needs at least two `files` parts, got {}",
                up.files.len()
            ),
        ));
    }
    for f in &up.files {
        validate_pdf(f)?;
    }
    let files = up.files;
    let pdf = blocking(move || service::run_merge(files)).await?;
    Ok(pdf_response(pdf))
}

#[utoipa::path(
    post,
    path = "/v1/edit/split",
    tag = "edit",
    request_body(content_type = "multipart/form-data", description = "Parts: `file` (PDF), `options` (JSON `{ ranges: [[start,end],…] }`, one-based inclusive)"),
    responses(
        (status = 200, content_type = "application/zip", description = "Zip with one PDF per range"),
        (status = 422, body = ApiError, description = "Range out of bounds")
    )
)]
pub async fn edit_split(mp: Multipart) -> Result<Response, ApiErr> {
    let mut up = collect(mp).await?;
    let bytes = up.one_file()?;
    let req: SplitRequest = up.require_options()?;
    let ranges: Vec<(usize, usize)> = req.ranges.iter().map(|r| (r[0], r[1])).collect();
    let parts = blocking(move || service::run_split(bytes, &ranges)).await?;
    let zipped = zip_parts(&parts)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"split.zip\"",
            ),
        ],
        zipped,
    )
        .into_response())
}

#[utoipa::path(
    post,
    path = "/v1/edit/rotate",
    tag = "edit",
    request_body(content_type = "multipart/form-data", description = "Parts: `file` (PDF), `options` (JSON `{ rotations: [{page, degrees}] }`)"),
    responses((status = 200, content_type = "application/pdf", description = "Rotated PDF"))
)]
pub async fn edit_rotate(mp: Multipart) -> Result<Response, ApiErr> {
    let mut up = collect(mp).await?;
    let bytes = up.one_file()?;
    let req: RotateRequest = up.require_options()?;
    let rotations: Vec<(usize, i32)> = req.rotations.iter().map(|r| (r.page, r.degrees)).collect();
    let pdf = blocking(move || service::run_rotate(bytes, &rotations)).await?;
    Ok(pdf_response(pdf))
}

#[utoipa::path(
    post,
    path = "/v1/edit/watermark",
    tag = "edit",
    request_body(content_type = "multipart/form-data", description = "Parts: `file` (PDF), `options` (JSON `{ text, font_size?, gray?, rotation_degrees? }`)"),
    responses((status = 200, content_type = "application/pdf", description = "Watermarked PDF"))
)]
pub async fn edit_watermark(mp: Multipart) -> Result<Response, ApiErr> {
    let mut up = collect(mp).await?;
    let bytes = up.one_file()?;
    let req: WatermarkRequest = up.require_options()?;
    let pdf = blocking(move || service::run_watermark(bytes, req)).await?;
    Ok(pdf_response(pdf))
}

#[utoipa::path(
    post,
    path = "/v1/edit/fill",
    tag = "edit",
    request_body(content_type = "multipart/form-data", description = "Parts: `file` (PDF), `options` (JSON `{ fields: {name: value} }`)"),
    responses((status = 200, content_type = "application/pdf", description = "Form-filled PDF"))
)]
pub async fn edit_fill(mp: Multipart) -> Result<Response, ApiErr> {
    let mut up = collect(mp).await?;
    let bytes = up.one_file()?;
    let req: FillRequest = up.require_options()?;
    let pdf = blocking(move || service::run_fill(bytes, req.fields)).await?;
    Ok(pdf_response(pdf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_pdf_with_leading_bytes() {
        // %PDF- need not be at offset 0 (PRD §13.5).
        let mut bytes = b"\x00\x00junk".to_vec();
        bytes.extend_from_slice(b"%PDF-1.7\n...");
        assert!(validate_pdf(&bytes).is_ok());
    }

    #[test]
    fn rejects_non_pdf() {
        assert!(validate_pdf(b"GET / HTTP/1.1\r\n").is_err());
        assert!(validate_pdf(b"").is_err());
    }

    #[test]
    fn zip_has_one_entry_per_part() {
        let parts = vec![b"%PDF-a".to_vec(), b"%PDF-b".to_vec()];
        let zipped = zip_parts(&parts).expect("zip");
        let reader = zip::ZipArchive::new(Cursor::new(zipped)).expect("read zip");
        assert_eq!(reader.len(), 2);
    }
}

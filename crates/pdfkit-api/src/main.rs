//! `pdfkit-api` — a self-hostable HTTP surface over the pdfkit library (PRD §13).
//!
//! This crate is a leaf binary, excluded from the workspace `default-members`
//! and from the library CI gates, so the async/network stack (axum/tokio) never
//! reaches the default or minimal library build (PRD §1 design rule 6).

mod dto;
mod error;
mod handlers;
mod service;

use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, StatusCode};
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;

/// The OpenAPI document, generated from the handler `#[utoipa::path]` attributes
/// and the DTO `ToSchema` derives (PRD §13.6). Served at `/openapi.json`.
#[derive(OpenApi)]
#[openapi(
    paths(
        healthz,
        version,
        handlers::extract,
        handlers::metadata,
        handlers::chunks,
        handlers::figures,
        handlers::render,
        handlers::edit_merge,
        handlers::edit_split,
        handlers::edit_rotate,
        handlers::edit_watermark,
        handlers::edit_fill,
        handlers::convert_docx,
    ),
    components(schemas(
        dto::ApiError,
        dto::ExtractResponse,
        dto::PageImage,
        dto::TruncatedDto,
        dto::MetadataResponse,
        dto::OutlineNode,
        dto::PageLinks,
        dto::LinkDto,
        dto::LinkTargetDto,
        dto::FiguresResponse,
        dto::PageFigures,
        dto::FigureDto,
    )),
    tags(
        (name = "read", description = "Read endpoints (extract, metadata, chunks, figures, render)"),
        (name = "edit", description = "Write endpoints (merge, split, rotate, watermark, fill)"),
        (name = "convert", description = "Conversion endpoints (Word .docx → PDF)"),
        (name = "meta", description = "Health and version")
    )
)]
struct ApiDoc;

const DEFAULT_MAX_BODY_BYTES: usize = 50 * 1024 * 1024;
const DEFAULT_TIMEOUT_SECS: u64 = 60;

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// CORS from `PDFKIT_ALLOWED_ORIGINS` (comma-separated origins, or `*` for any).
/// Default (unset/empty) DENIES cross-origin — a safe default for a self-hosted
/// service; set the env var to allow a frontend (PRD §13.5).
fn cors_layer() -> CorsLayer {
    let raw = std::env::var("PDFKIT_ALLOWED_ORIGINS").unwrap_or_default();
    let raw = raw.trim();

    if raw.is_empty() {
        eprintln!(
            "pdfkit-api: PDFKIT_ALLOWED_ORIGINS is unset — cross-origin requests are denied. \
             Set it to a comma-separated origin list (or `*`) to allow a frontend."
        );
        return CorsLayer::new();
    }

    // `*` cannot be passed to AllowOrigin::list (it panics); map it to Any.
    if raw == "*" {
        return CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);
    }

    let mut origins: Vec<HeaderValue> = Vec::new();
    for o in raw.split(',').map(str::trim).filter(|o| !o.is_empty()) {
        match o.parse::<HeaderValue>() {
            Ok(v) => origins.push(v),
            Err(_) => {
                eprintln!("pdfkit-api: ignoring invalid origin in PDFKIT_ALLOWED_ORIGINS: {o}")
            }
        }
    }

    if origins.is_empty() {
        eprintln!(
            "pdfkit-api: no valid origins parsed from PDFKIT_ALLOWED_ORIGINS — cross-origin requests are denied."
        );
        return CorsLayer::new();
    }

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods(Any)
        .allow_headers(Any)
}

/// Build the application router. Kept separate from `main` so it is testable
/// without binding a socket.
fn app() -> Router {
    let max_body = env_parse("PDFKIT_MAX_BODY_BYTES", DEFAULT_MAX_BODY_BYTES);
    let timeout = env_parse("PDFKIT_REQUEST_TIMEOUT_SECS", DEFAULT_TIMEOUT_SECS);

    Router::new()
        .route("/healthz", get(healthz))
        .route("/version", get(version))
        .route("/openapi.json", get(openapi_json))
        .route("/docs", get(docs))
        .route("/v1/extract", post(handlers::extract))
        .route("/v1/metadata", post(handlers::metadata))
        .route("/v1/chunks", post(handlers::chunks))
        .route("/v1/figures", post(handlers::figures))
        .route("/v1/render", post(handlers::render))
        .route("/v1/edit/merge", post(handlers::edit_merge))
        .route("/v1/edit/split", post(handlers::edit_split))
        .route("/v1/edit/rotate", post(handlers::edit_rotate))
        .route("/v1/edit/watermark", post(handlers::edit_watermark))
        .route("/v1/edit/fill", post(handlers::edit_fill))
        .route("/v1/convert/docx-to-pdf", post(handlers::convert_docx))
        .layer(DefaultBodyLimit::max(max_body))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(timeout),
        ))
        .layer(cors_layer())
        .layer(TraceLayer::new_for_http())
}

/// Liveness probe.
#[utoipa::path(get, path = "/healthz", tag = "meta", responses((status = 200, description = "Liveness probe")))]
async fn healthz() -> &'static str {
    "ok"
}

/// Service name, version, and the set of compiled-in optional features.
#[utoipa::path(get, path = "/version", tag = "meta", responses((status = 200, description = "Service version and compiled feature flags")))]
async fn version() -> Json<Value> {
    Json(json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "features": {
            "render_pdfium": cfg!(feature = "render-pdfium"),
            "ocr": cfg!(feature = "ocr"),
            "docx": cfg!(feature = "docx"),
        },
    }))
}

/// The generated OpenAPI document as JSON.
async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

/// Swagger UI, loading assets from a CDN and pointing at `/openapi.json`.
async fn docs() -> Html<&'static str> {
    Html(SWAGGER_HTML)
}

const SWAGGER_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <title>pdfkit-api — API docs</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist/swagger-ui.css" />
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist/swagger-ui-bundle.js"></script>
  <script>
    window.onload = () => {
      window.ui = SwaggerUIBundle({ url: "/openapi.json", dom_id: "#swagger-ui" });
    };
  </script>
</body>
</html>"##;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber_init();
    let addr = std::env::var("PDFKIT_API_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    eprintln!("pdfkit-api listening on http://{addr}");
    axum::serve(listener, app()).await
}

/// No-op subscriber hook for now; structured logging is wired in a later step.
/// `TraceLayer` is a no-op without a subscriber, which is fine.
fn tracing_subscriber_init() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_builds() {
        let _ = app();
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        assert_eq!(healthz().await, "ok");
    }

    #[tokio::test]
    async fn version_reports_name_and_feature_flags() {
        let Json(v) = version().await;
        assert_eq!(v["name"], "pdfkit-api");
        assert!(v["version"].is_string());
        assert!(v["features"]["render_pdfium"].is_boolean());
        assert!(v["features"]["ocr"].is_boolean());
        assert!(v["features"]["docx"].is_boolean());
    }
}

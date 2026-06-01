//! API error type and the `PdfError` → HTTP mapping (PRD §13.4).
//!
//! Backends are already collapsed into `PdfError` at the library boundary, so
//! handlers just `?`-propagate. Backend detail and passwords are never echoed
//! into the response body.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use pdfkit_core::PdfError;

use crate::dto::ApiError;

/// An error that can be returned from a handler as a JSON [`ApiError`] envelope.
#[derive(Debug)]
pub struct ApiErr {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiErr {
    /// A 400 with a machine-readable code.
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        ApiErr {
            status: StatusCode::BAD_REQUEST,
            code,
            message: message.into(),
        }
    }

    /// A 501 (e.g. rendering on a build without the `render-pdfium` feature).
    /// Only called by the non-pdfium render stub, so it is unused when that
    /// feature is on.
    #[cfg_attr(feature = "render-pdfium", allow(dead_code))]
    pub fn not_implemented(message: impl Into<String>) -> Self {
        ApiErr {
            status: StatusCode::NOT_IMPLEMENTED,
            code: "not_implemented",
            message: message.into(),
        }
    }

    /// A 500 for unexpected server-side failures (e.g. a worker join error).
    pub fn internal(message: impl Into<String>) -> Self {
        ApiErr {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal",
            message: message.into(),
        }
    }
}

impl From<PdfError> for ApiErr {
    fn from(e: PdfError) -> Self {
        let (status, code) = match &e {
            PdfError::Format(_) => (StatusCode::BAD_REQUEST, "malformed_pdf"),
            PdfError::Password => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "password_required_or_incorrect",
            ),
            PdfError::Security => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "unsupported_security_handler",
            ),
            PdfError::PageRange(_) => (StatusCode::UNPROCESSABLE_ENTITY, "page_out_of_range"),
            PdfError::Budget => (StatusCode::PAYLOAD_TOO_LARGE, "budget_exceeded"),
            PdfError::Backend(_) => (StatusCode::BAD_GATEWAY, "backend_error"),
            // `PdfError` is #[non_exhaustive]; anything else is an internal fault.
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        };
        // `Backend` can carry arbitrary internal detail, so don't echo it; the
        // other variants' Display strings are safe and useful to the caller.
        let message = match &e {
            PdfError::Backend(_) => "backend error".to_string(),
            other => other.to_string(),
        };
        ApiErr {
            status,
            code,
            message,
        }
    }
}

impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiError {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_maps_to_422() {
        let e = ApiErr::from(PdfError::Password);
        assert_eq!(e.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(e.code, "password_required_or_incorrect");
    }

    #[test]
    fn budget_maps_to_413() {
        assert_eq!(
            ApiErr::from(PdfError::Budget).status,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[test]
    fn page_range_maps_to_422() {
        assert_eq!(
            ApiErr::from(PdfError::PageRange(99)).status,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[test]
    fn backend_detail_is_not_leaked() {
        let e = ApiErr::from(PdfError::Backend("secret path /etc/passwd".into()));
        assert_eq!(e.status, StatusCode::BAD_GATEWAY);
        assert!(!e.message.contains("secret"));
        assert!(!e.message.contains("/etc"));
    }
}

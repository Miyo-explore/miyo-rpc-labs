//! Error types for Miyo RPC Labs.
//!
//! These errors are intentionally structured so callers can distinguish
//! between transport-level failures, JSON-RPC protocol errors, and
//! validation problems — each of which requires a different remediation.

use thiserror::Error;

/// Top-level result alias used across the crate.
pub type Result<T> = std::result::Result<T, MiyoError>;

/// All error variants that can arise during an RPC diagnostic run.
#[derive(Debug, Error, Clone)]
pub enum MiyoError {
    /// The endpoint URL was missing, malformed, or contained an unsupported scheme.
    #[error("invalid endpoint URL: {0}")]
    InvalidUrl(String),

    /// The HTTP request could not be completed (DNS, connection refused, timeout, etc.).
    #[error("network failure: {0}")]
    Network(String),

    /// The HTTP response had a non-200 status code.
    ///
    /// `retry_after` carries the raw `Retry-After` response header when the
    /// server provided one for a 429 response. It is preserved verbatim (it may
    /// be either a delta-seconds integer or an HTTP-date) and is never invented.
    #[error("HTTP {status}: {body}")]
    HttpError {
        status: u16,
        body: String,
        retry_after: Option<String>,
    },

    /// The response body was not valid JSON.
    #[error("invalid JSON response")]
    InvalidJson { raw: String },

    /// The JSON-RPC response structure was malformed (missing required fields,
    /// wrong types, etc.).
    #[error("malformed JSON-RPC response: {0}")]
    MalformedStructure(String),

    /// The JSON-RPC server returned a well-formed error object.
    #[error("JSON-RPC error {code}: {message}")]
    JsonRpcError {
        code: i32,
        message: String,
        data: Option<String>,
    },

    /// A Stellar-specific RPC method returned an unexpected or unsupported result.
    #[error("unsupported or unexpected response from method '{0}': {1}")]
    UnsupportedMethod(String, String),

    /// The operation timed out.
    #[error("request timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// A required field was missing or had an incorrect type.
    #[error("missing or invalid field: {0}")]
    MissingField(String),

    /// Any other error not covered above.
    #[error("{0}")]
    Other(String),
}

impl MiyoError {
    /// Categorize this error into a coarse bucket for reporting.
    pub fn category(&self) -> ErrorCategory {
        match self {
            MiyoError::Network(_) | MiyoError::Timeout(_) => ErrorCategory::Transport,
            // 429 is distinct from other 4xx: the endpoint is reachable and
            // configured correctly, but is throttling this client. Retrieved
            // before the generic 4xx arm so it is not bucketed as ClientError.
            MiyoError::HttpError { status, .. } if *status == 429 => ErrorCategory::RateLimited,
            MiyoError::HttpError { status, .. } if *status >= 500 => ErrorCategory::ServerError,
            MiyoError::HttpError { status, .. } if *status >= 400 => ErrorCategory::ClientError,
            MiyoError::HttpError { .. } => ErrorCategory::Transport,
            MiyoError::InvalidJson { .. } => ErrorCategory::MalformedResponse,
            MiyoError::MalformedStructure(_) => ErrorCategory::MalformedResponse,
            MiyoError::JsonRpcError { code, .. } if *code == -32601 => {
                ErrorCategory::UnsupportedMethod
            }
            MiyoError::JsonRpcError { .. } => ErrorCategory::JsonRpcError,
            MiyoError::UnsupportedMethod(_, _) => ErrorCategory::UnsupportedMethod,
            MiyoError::MissingField(_) => ErrorCategory::MalformedResponse,
            MiyoError::InvalidUrl(_) => ErrorCategory::Configuration,
            MiyoError::Other(_) => ErrorCategory::Unknown,
        }
    }
}

/// Fine-grained provider error shape for a failure, surfaced as
/// `error_shape` in a failed check's `details`.
///
/// This is finer than the coarse [`IssueCategory`] bucket (which never
/// changes): two failures in the same category can have different shapes
/// — e.g. a 5xx carrying a Cloudflare HTML error page (`html_error_page`)
/// versus a 5xx carrying a JSON body (`http_server_error`). The shape
/// names *how* the provider failed, so a reader can tell providers apart
/// without opening the raw response. It never changes the exit code.
///
/// The mapping is table-driven where the discriminator is a code
/// ([`JSON_RPC_CODE_SHAPES`]); HTTP statuses are range-matched, and the
/// body is inspected only for two shapes the client itself can observe:
/// an HTML error page, and the client's own oversized-body message. No
/// provider-specific string matching beyond that: the taxonomy classifies
/// shapes, it does not fingerprint providers.
pub fn classify_error_shape(err: &MiyoError) -> &'static str {
    match err {
        MiyoError::HttpError { status, body, .. } => {
            if is_oversized_body(body) {
                "oversized_response"
            } else if *status == 429 {
                "rate_limited"
            } else if is_html_body(body) {
                "html_error_page"
            } else if *status >= 500 {
                "http_server_error"
            } else if *status >= 400 {
                "http_client_error"
            } else {
                "http_error"
            }
        }
        MiyoError::JsonRpcError { code, .. } => JSON_RPC_CODE_SHAPES
            .iter()
            .find(|(c, _)| *c == *code)
            .map(|(_, shape)| *shape)
            .unwrap_or("json_rpc_error"),
        MiyoError::InvalidJson { .. } => "non_json_response",
        MiyoError::MalformedStructure(_) => "malformed_response",
        MiyoError::UnsupportedMethod(_, _) => "unsupported_method",
        MiyoError::MissingField(_) => "missing_field",
        MiyoError::Network(_) => "transport_error",
        MiyoError::Timeout(_) => "timeout",
        MiyoError::InvalidUrl(_) => "configuration_error",
        MiyoError::Other(_) => "unknown_error",
    }
}

/// JSON-RPC error codes with a dedicated shape. Any other code maps to
/// `"json_rpc_error"`.
const JSON_RPC_CODE_SHAPES: &[(i32, &str)] = &[
    (-32601, "method_not_found"),
    (-32602, "invalid_params"),
    (-32603, "internal_error"),
];

/// Whether `body` is the client's own oversized-response message (see
/// `RpcClient::call_raw`): the body is synthetic, not provider text, so
/// matching its fixed prefix matches our own format, not a provider.
fn is_oversized_body(body: &str) -> bool {
    body.starts_with("response body exceeds maximum size")
}

/// Whether `body` looks like an HTML error page (Cloudflare-style):
/// leading whitespace ignored, case-insensitive `<!doctype html` or
/// `<html` opening tag.
fn is_html_body(body: &str) -> bool {
    let t = body.trim_start();
    t.get(..14)
        .is_some_and(|h| h.eq_ignore_ascii_case("<!doctype html"))
        || t.get(..5).is_some_and(|h| h.eq_ignore_ascii_case("<html"))
}

/// Coarse-grained error category for reporting purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// DNS resolution, connection refused, timeout, etc.
    Transport,
    /// HTTP 5xx from the server.
    ServerError,
    /// HTTP 4xx (not JSON-RPC errors).
    ClientError,
    /// HTTP 429: the endpoint is reachable but is rate-limiting this client.
    RateLimited,
    /// Response body was not parseable JSON.
    MalformedResponse,
    /// A well-formed JSON-RPC -32601 error: method not found / unsupported.
    UnsupportedMethod,
    /// Any other JSON-RPC error (-32600, -32602, -32603, or server-specific codes).
    JsonRpcError,
    /// The URL or configuration was invalid.
    Configuration,
    /// Anything else.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(status: u16) -> MiyoError {
        MiyoError::HttpError {
            status,
            body: String::new(),
            retry_after: None,
        }
    }

    #[test]
    fn test_429_is_rate_limited() {
        assert_eq!(http(429).category(), ErrorCategory::RateLimited);
    }

    #[test]
    fn test_404_is_client_error() {
        assert_eq!(http(404).category(), ErrorCategory::ClientError);
    }

    #[test]
    fn test_500_is_server_error() {
        assert_eq!(http(500).category(), ErrorCategory::ServerError);
    }

    #[test]
    fn test_503_is_server_error() {
        assert_eq!(http(503).category(), ErrorCategory::ServerError);
    }

    #[test]
    fn test_other_4xx_remain_client_error() {
        for status in [400, 401, 403, 418, 499] {
            assert_eq!(
                http(status).category(),
                ErrorCategory::ClientError,
                "status {status} must stay ClientError"
            );
        }
    }

    #[test]
    fn test_3xx_stays_transport() {
        assert_eq!(http(302).category(), ErrorCategory::Transport);
    }

    fn http_body(status: u16, body: &str) -> MiyoError {
        MiyoError::HttpError {
            status,
            body: body.to_string(),
            retry_after: None,
        }
    }

    fn rpc_error(code: i32) -> MiyoError {
        MiyoError::JsonRpcError {
            code,
            message: "error".to_string(),
            data: None,
        }
    }

    #[test]
    fn test_error_shape_json_rpc_codes_table() {
        assert_eq!(classify_error_shape(&rpc_error(-32601)), "method_not_found");
        assert_eq!(classify_error_shape(&rpc_error(-32602)), "invalid_params");
        assert_eq!(classify_error_shape(&rpc_error(-32603)), "internal_error");
        // Any other code falls back to the generic shape.
        assert_eq!(classify_error_shape(&rpc_error(-32000)), "json_rpc_error");
        assert_eq!(classify_error_shape(&rpc_error(0)), "json_rpc_error");
    }

    #[test]
    fn test_error_shape_http_statuses() {
        assert_eq!(
            classify_error_shape(&http_body(429, "slow down")),
            "rate_limited"
        );
        assert_eq!(
            classify_error_shape(&http_body(500, "{\"error\":\"boom\"}")),
            "http_server_error"
        );
        assert_eq!(
            classify_error_shape(&http_body(503, "unavailable")),
            "http_server_error"
        );
        assert_eq!(
            classify_error_shape(&http_body(404, "not found")),
            "http_client_error"
        );
        assert_eq!(
            classify_error_shape(&http_body(403, "forbidden")),
            "http_client_error"
        );
    }

    #[test]
    fn test_error_shape_html_error_page() {
        assert_eq!(
            classify_error_shape(&http_body(
                503,
                "<!DOCTYPE html><html><body>error</body></html>"
            )),
            "html_error_page"
        );
        assert_eq!(
            classify_error_shape(&http_body(403, "  <html><head></head></html>")),
            "html_error_page"
        );
        assert_eq!(
            classify_error_shape(&http_body(502, "<!DoCtYpE HtMl>")),
            "html_error_page"
        );
        // A JSON body on a 5xx is not an HTML page.
        assert_eq!(
            classify_error_shape(&http_body(500, "{\"code\": 500}")),
            "http_server_error"
        );
    }

    #[test]
    fn test_error_shape_oversized_response() {
        assert_eq!(
            classify_error_shape(&http_body(
                200,
                "response body exceeds maximum size of 100 bytes (got 500)"
            )),
            "oversized_response"
        );
        // The oversize prefix wins over the status: the synthetic body
        // is the signal, even on a 200.
        assert_eq!(
            classify_error_shape(&http_body(
                500,
                "response body exceeds maximum size of 100 bytes (got 500)"
            )),
            "oversized_response"
        );
    }

    #[test]
    fn test_error_shape_other_variants() {
        assert_eq!(
            classify_error_shape(&MiyoError::InvalidJson {
                raw: "<10-byte body not shown>".to_string()
            }),
            "non_json_response"
        );
        assert_eq!(
            classify_error_shape(&MiyoError::MalformedStructure(
                "missing 'result'".to_string()
            )),
            "malformed_response"
        );
        assert_eq!(
            classify_error_shape(&MiyoError::UnsupportedMethod(
                "simulateTransaction".to_string(),
                "simulation failed".to_string()
            )),
            "unsupported_method"
        );
        assert_eq!(
            classify_error_shape(&MiyoError::MissingField("sequence".to_string())),
            "missing_field"
        );
        assert_eq!(
            classify_error_shape(&MiyoError::Network("connection failed".to_string())),
            "transport_error"
        );
        assert_eq!(
            classify_error_shape(&MiyoError::Timeout(std::time::Duration::from_secs(5))),
            "timeout"
        );
        assert_eq!(
            classify_error_shape(&MiyoError::InvalidUrl("bad scheme".to_string())),
            "configuration_error"
        );
        assert_eq!(
            classify_error_shape(&MiyoError::Other("something else".to_string())),
            "unknown_error"
        );
    }
}

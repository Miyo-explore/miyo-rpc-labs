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
    #[error("HTTP {status}: {body}")]
    HttpError { status: u16, body: String },

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

/// Coarse-grained error category for reporting purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// DNS resolution, connection refused, timeout, etc.
    Transport,
    /// HTTP 5xx from the server.
    ServerError,
    /// HTTP 4xx (not JSON-RPC errors).
    ClientError,
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

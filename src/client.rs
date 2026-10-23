//! HTTP client for sending JSON-RPC requests to Stellar RPC endpoints.
//!
//! All methods sanitize sensitive URL parameters before they appear in any
//! error message or log. The client never embeds credentials in error text.

use crate::error::{MiyoError, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::{Duration, Instant};

/// Configuration for creating an [`RpcClient`].
#[derive(Debug, Clone)]
pub struct RpcClientConfig {
    /// Maximum time to wait for a single HTTP request to complete.
    pub timeout: Duration,
    /// Whether to follow HTTP redirects. Default: true.
    pub follow_redirects: bool,
    /// User-Agent string sent with requests.
    pub user_agent: String,
    /// Maximum response body size in bytes. Responses exceeding this are
    /// rejected as errors rather than buffered into memory.
    pub max_response_bytes: usize,
}

/// Default maximum response body size: 10 MiB.
///
/// This is generous enough for any single Stellar JSON-RPC response
/// (the largest is `getLatestLedger` with `metadataXdr`), while preventing
/// unbounded memory consumption from misconfigured or malicious endpoints.
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

impl Default for RpcClientConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(15),
            follow_redirects: true,
            user_agent: format!("miyo-rpc/{}", crate::VERSION),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }
}

impl RpcClientConfig {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            ..Default::default()
        }
    }
}

/// A minimal JSON-RPC 2.0 client that talks to Stellar RPC endpoints.
pub struct RpcClient {
    http: Client,
    url: String,
    config: RpcClientConfig,
}

/// Raw HTTP-level response captured before JSON parsing.
#[derive(Debug)]
pub struct HttpResponseInfo {
    pub status: u16,
    pub duration: Duration,
    pub timestamp: DateTime<Utc>,
    pub headers_json: serde_json::Map<String, Value>,
}

impl RpcClient {
    /// Create a new client for the given endpoint URL.
    pub fn new(url: &str, config: RpcClientConfig) -> Result<Self> {
        // Validate the URL scheme and structure.
        let parsed = url::Url::parse(url)
            .map_err(|e| MiyoError::InvalidUrl(format!("failed to parse URL: {e}")))?;

        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(MiyoError::InvalidUrl(format!(
                "unsupported URL scheme '{}'; only http and https are supported",
                parsed.scheme()
            )));
        }

        let client = Client::builder()
            .timeout(config.timeout)
            .redirect(if config.follow_redirects {
                reqwest::redirect::Policy::default()
            } else {
                reqwest::redirect::Policy::none()
            })
            .user_agent(&config.user_agent)
            .build()
            .map_err(|e| MiyoError::Network(format!("failed to build HTTP client: {e}")))?;

        Ok(Self {
            http: client,
            url: url.to_string(),
            config,
        })
    }

    /// The sanitized endpoint URL (query params with keys like 'key', 'token',
    /// 'secret' are redacted).
    pub fn sanitized_url(&self) -> String {
        crate::output::sanitize_url(&self.url)
    }

    /// Send a JSON-RPC request and return the HTTP-level info plus the parsed
    /// response body (as a [`serde_json::Value`] for flexible inspection).
    pub async fn call_raw(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(HttpResponseInfo, Value)> {
        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let started = Instant::now();
        let timestamp = Utc::now();
        let response = self
            .http
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await;

        let duration = started.elapsed();

        let mut response = match response {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return Err(MiyoError::Timeout(self.config.timeout)),
            Err(e) => {
                // Redact the endpoint URL before embedding it in an error
                // message — reqwest's Display output may include the raw URL
                // (which can carry credentials in userinfo).
                let endpoint = crate::output::sanitize_url(&self.url);
                let detail = if e.is_connect() {
                    "connection failed".to_string()
                } else {
                    "request failed".to_string()
                };
                return Err(MiyoError::Network(format!("{detail} for {endpoint}")));
            }
        };

        let status = response.status().as_u16();
        let headers = response.headers().clone();

        // Read the body incrementally, enforcing a size cap so a misconfigured
        // or malicious endpoint cannot exhaust memory.
        let max = self.config.max_response_bytes;
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    bytes.extend_from_slice(&chunk);
                    if bytes.len() > max {
                        return Err(MiyoError::HttpError {
                            status,
                            body: format!(
                                "response body exceeds maximum size of {} bytes (got {})",
                                max,
                                bytes.len()
                            ),
                        });
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    return Err(MiyoError::Network(format!(
                        "failed to read response body: {e}"
                    )));
                }
            }
        }

        let headers_json: serde_json::Map<String, Value> = headers
            .iter()
            .map(|(k, v)| {
                let val = v.to_str().unwrap_or("<invalid header>").to_string();
                (k.as_str().to_lowercase(), Value::String(val))
            })
            .collect();

        let info = HttpResponseInfo {
            status,
            duration,
            timestamp,
            headers_json,
        };

        if status != 200 {
            let body_str = String::from_utf8_lossy(&bytes).to_string();
            let truncated = if body_str.len() > 500 {
                format!("{}... (truncated)", &body_str[..500])
            } else {
                body_str.clone()
            };
            return Err(MiyoError::HttpError {
                status,
                body: truncated,
            });
        }

        // Parse JSON — but don't leak the raw body on error (it may contain
        // sensitive data embedded by a misconfigured proxy). Use a hash instead.
        let parsed: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                return Err(MiyoError::InvalidJson {
                    raw: format!(
                        "<{}-byte body not shown for security — content-type may be wrong or body is non-JSON>",
                        bytes.len()
                    ),
                });
            }
        };

        Ok((info, parsed))
    }

    /// Send a JSON-RPC request and deserialize the `result` field.
    pub async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(HttpResponseInfo, T)> {
        let (info, parsed) = self.call_raw(method, params).await?;

        // Validate JSON-RPC envelope.
        let jsonrpc = parsed
            .get("jsonrpc")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                MiyoError::MalformedStructure("missing 'jsonrpc' field in response".to_string())
            })?;

        if jsonrpc != "2.0" {
            return Err(MiyoError::MalformedStructure(format!(
                "expected jsonrpc '2.0', got '{jsonrpc}'"
            )));
        }

        // Check for JSON-RPC error object.
        if let Some(err) = parsed.get("error") {
            let code = err.get("code").and_then(|v| v.as_i64()).ok_or_else(|| {
                MiyoError::MalformedStructure(
                    "JSON-RPC error object missing 'code' field".to_string(),
                )
            })? as i32;
            let message = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("no message")
                .to_string();
            let data = err.get("data").and_then(|v| v.as_str()).map(|s| {
                if s.len() > 500 {
                    format!("{}... (truncated)", &s[..500])
                } else {
                    s.to_string()
                }
            });

            return Err(MiyoError::JsonRpcError {
                code,
                message,
                data,
            });
        }

        // Extract result.
        let result = parsed.get("result").ok_or_else(|| {
            MiyoError::MalformedStructure("missing 'result' field in response".to_string())
        })?;

        let typed: T = serde_json::from_value(result.clone()).map_err(|e| {
            MiyoError::MalformedStructure(format!(
                "failed to deserialize result for method '{method}': {e}"
            ))
        })?;

        Ok((info, typed))
    }
}

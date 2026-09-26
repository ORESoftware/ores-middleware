use std::{collections::BTreeMap, net::IpAddr};

use serde::Serialize;

use crate::{
    hardening::{MAX_HEADER_BYTES, MAX_HEADER_NAME_BYTES, MAX_HEADER_VALUE_BYTES, MAX_RAW_HEADERS},
    net::cidr_contains,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpIngressAdapter {
    Standalone,
    Scintilla,
    AwsLambdaHttp,
    GcpCloudRun,
    GcpCloudRunFunction,
}

pub const CERTIFIED_HTTP_INGRESS_ADAPTERS: &[HttpIngressAdapter] = &[
    HttpIngressAdapter::Standalone,
    HttpIngressAdapter::Scintilla,
    HttpIngressAdapter::AwsLambdaHttp,
    HttpIngressAdapter::GcpCloudRun,
    HttpIngressAdapter::GcpCloudRunFunction,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestBufferLimits {
    pub max_header_count: usize,
    pub max_header_bytes: usize,
    pub max_body_bytes: u64,
    pub max_upload_bytes: u64,
    pub max_decompressed_bytes: u64,
    pub max_decompression_ratio: u64,
}

impl RequestBufferLimits {
    #[must_use]
    pub fn from_body_limit(max_body_bytes: usize) -> Self {
        let body = u64::try_from(max_body_bytes).unwrap_or(u64::MAX);
        return Self {
            max_header_count: MAX_RAW_HEADERS,
            max_header_bytes: MAX_HEADER_BYTES,
            max_body_bytes: body,
            max_upload_bytes: body,
            max_decompressed_bytes: body,
            max_decompression_ratio: 32,
        };
    }

    pub fn validate(&self) -> Result<(), BufferAdmissionError> {
        if self.max_header_count == 0
            || self.max_header_bytes == 0
            || self.max_body_bytes == 0
            || self.max_upload_bytes == 0
            || self.max_decompressed_bytes == 0
            || self.max_decompression_ratio == 0
        {
            return Err(BufferAdmissionError::InvalidLimits);
        }
        return Ok(());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestBufferMetadata {
    pub declared_body_bytes: Option<u64>,
    pub upload: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferAdmissionError {
    InvalidLimits,
    HeaderCountExceeded,
    HeaderBytesExceeded,
    HeaderNameInvalid,
    HeaderValueExceeded,
    BodyExceeded,
    UploadExceeded,
    DecompressedExceeded,
    DecompressionRatioExceeded,
}

/// Checks request-head information before a body, multipart upload, or
/// decompressor is allowed to allocate a large buffer.
pub fn preflight_request_buffers(
    limits: RequestBufferLimits,
    raw_headers: &[(&str, &str)],
    metadata: RequestBufferMetadata,
) -> Result<(), BufferAdmissionError> {
    limits.validate()?;
    if raw_headers.len() > limits.max_header_count {
        return Err(BufferAdmissionError::HeaderCountExceeded);
    }

    let mut header_bytes = 0usize;
    for (name, value) in raw_headers {
        if name.is_empty() || name.len() > MAX_HEADER_NAME_BYTES {
            return Err(BufferAdmissionError::HeaderNameInvalid);
        }
        if value.len() > MAX_HEADER_VALUE_BYTES {
            return Err(BufferAdmissionError::HeaderValueExceeded);
        }
        header_bytes = header_bytes
            .checked_add(name.len())
            .and_then(|size| size.checked_add(value.len()))
            .ok_or(BufferAdmissionError::HeaderBytesExceeded)?;
        if header_bytes > limits.max_header_bytes {
            return Err(BufferAdmissionError::HeaderBytesExceeded);
        }
    }

    if let Some(bytes) = metadata.declared_body_bytes {
        if bytes > limits.max_body_bytes {
            return Err(BufferAdmissionError::BodyExceeded);
        }
        if metadata.upload && bytes > limits.max_upload_bytes {
            return Err(BufferAdmissionError::UploadExceeded);
        }
    }
    return Ok(());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecompressionBudget {
    limits: RequestBufferLimits,
    compressed_bytes: u64,
    decompressed_bytes: u64,
}

impl DecompressionBudget {
    pub fn new(limits: RequestBufferLimits) -> Result<Self, BufferAdmissionError> {
        limits.validate()?;
        return Ok(Self {
            limits,
            compressed_bytes: 0,
            decompressed_bytes: 0,
        });
    }

    /// Call before retaining expanded output. Rejected chunks must be discarded.
    pub fn admit_chunk(
        &mut self,
        compressed_bytes: u64,
        decompressed_bytes: u64,
    ) -> Result<(), BufferAdmissionError> {
        let next_compressed = self
            .compressed_bytes
            .checked_add(compressed_bytes)
            .ok_or(BufferAdmissionError::DecompressionRatioExceeded)?;
        let next_decompressed = self
            .decompressed_bytes
            .checked_add(decompressed_bytes)
            .ok_or(BufferAdmissionError::DecompressedExceeded)?;

        if next_decompressed > self.limits.max_decompressed_bytes {
            return Err(BufferAdmissionError::DecompressedExceeded);
        }
        if next_compressed == 0 {
            if next_decompressed != 0 {
                return Err(BufferAdmissionError::DecompressionRatioExceeded);
            }
        } else {
            let maximum = next_compressed
                .checked_mul(self.limits.max_decompression_ratio)
                .ok_or(BufferAdmissionError::DecompressionRatioExceeded)?;
            if next_decompressed > maximum {
                return Err(BufferAdmissionError::DecompressionRatioExceeded);
            }
        }

        self.compressed_bytes = next_compressed;
        self.decompressed_bytes = next_decompressed;
        return Ok(());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicProblem {
    #[serde(rename = "type")]
    pub problem_type: &'static str,
    pub title: &'static str,
    pub status: u16,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// `internal_diagnostic` is accepted deliberately so parity tests prove it
/// never crosses the public standalone/function boundary.
pub fn public_problem(
    _adapter: HttpIngressAdapter,
    status: u16,
    code: &str,
    request_id: Option<&str>,
    _internal_diagnostic: Option<&str>,
) -> PublicProblem {
    return PublicProblem {
        problem_type: "about:blank",
        title: "Request rejected",
        status,
        code: sanitize_problem_code(code),
        request_id: request_id.and_then(sanitize_request_id),
    };
}

#[must_use]
pub fn encode_public_problem(problem: &PublicProblem) -> Vec<u8> {
    return serde_json::to_vec(problem)
        .unwrap_or_else(|_| b"{\"title\":\"Request rejected\"}".to_vec());
}

fn sanitize_problem_code(value: &str) -> String {
    let admitted = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if admitted {
        return value.to_owned();
    }
    return "middleware_rejection".to_owned();
}

fn sanitize_request_id(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > 128 {
        return None;
    }
    return value
        .bytes()
        .all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
        .then(|| value.to_owned());
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedRequest<'a> {
    pub socket_peer: IpAddr,
    pub socket_host: &'a str,
    pub socket_scheme: &'a str,
    pub forwarded_host: Option<&'a str>,
    pub forwarded_scheme: Option<&'a str>,
    pub forwarded_client: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRequestAuthority {
    pub host: String,
    pub scheme: String,
    pub client_ip: IpAddr,
    pub used_forwarded_metadata: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedProxyPolicy {
    pub trusted_proxy_cidrs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyAdmissionError {
    UntrustedForwardingPeer,
    InvalidForwardedHost,
    InvalidForwardedScheme,
    InvalidForwardedClient,
}

impl TrustedProxyPolicy {
    pub fn resolve(
        &self,
        request: ForwardedRequest<'_>,
    ) -> Result<EffectiveRequestAuthority, ProxyAdmissionError> {
        let has_forwarding = request.forwarded_host.is_some()
            || request.forwarded_scheme.is_some()
            || request.forwarded_client.is_some();
        let trusted = self
            .trusted_proxy_cidrs
            .iter()
            .any(|cidr| cidr_contains(cidr, request.socket_peer));
        if has_forwarding && !trusted {
            return Err(ProxyAdmissionError::UntrustedForwardingPeer);
        }

        let host = match request.forwarded_host {
            Some(value) => validate_forwarded_host(value)?,
            None => validate_forwarded_host(request.socket_host)?,
        };
        let scheme = match request.forwarded_scheme {
            Some(value) => validate_forwarded_scheme(value)?,
            None => validate_forwarded_scheme(request.socket_scheme)?,
        };
        let client_ip = match request.forwarded_client {
            Some(value) => value
                .parse::<IpAddr>()
                .map_err(|_| ProxyAdmissionError::InvalidForwardedClient)?,
            None => request.socket_peer,
        };

        return Ok(EffectiveRequestAuthority {
            host,
            scheme,
            client_ip,
            used_forwarded_metadata: has_forwarding,
        });
    }
}

fn validate_forwarded_host(value: &str) -> Result<String, ProxyAdmissionError> {
    if value.is_empty()
        || value.len() > 253
        || value.contains(char::is_whitespace)
        || value.contains('/')
        || value.contains('@')
        || value.contains(',')
    {
        return Err(ProxyAdmissionError::InvalidForwardedHost);
    }
    return Ok(value.to_ascii_lowercase());
}

fn validate_forwarded_scheme(value: &str) -> Result<String, ProxyAdmissionError> {
    let normalized = value.to_ascii_lowercase();
    if !matches!(normalized.as_str(), "http" | "https") {
        return Err(ProxyAdmissionError::InvalidForwardedScheme);
    }
    return Ok(normalized);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserSecurityPolicy {
    pub allowed_origins: Vec<String>,
    pub allow_credentials: bool,
    pub require_csrf_for_cookie_mutations: bool,
    pub require_secure_session_cookies: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserRequest<'a> {
    pub method: &'a str,
    pub origin: Option<&'a str>,
    pub has_session_cookie: bool,
    pub csrf_verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserAdmissionError {
    OriginDenied,
    CsrfRequired,
    CredentialedWildcardOrigin,
    InsecureSessionCookie,
}

impl BrowserSecurityPolicy {
    pub fn validate(&self) -> Result<(), BrowserAdmissionError> {
        if self.allow_credentials && self.allowed_origins.iter().any(|origin| origin == "*") {
            return Err(BrowserAdmissionError::CredentialedWildcardOrigin);
        }
        return Ok(());
    }

    pub fn admit_request(
        &self,
        _adapter: HttpIngressAdapter,
        request: BrowserRequest<'_>,
    ) -> Result<(), BrowserAdmissionError> {
        self.validate()?;
        if let Some(origin) = request.origin {
            let allowed = self
                .allowed_origins
                .iter()
                .any(|candidate| candidate == "*" || candidate == origin);
            if !allowed {
                return Err(BrowserAdmissionError::OriginDenied);
            }
        }
        if self.require_csrf_for_cookie_mutations
            && request.has_session_cookie
            && unsafe_method(request.method)
            && !request.csrf_verified
        {
            return Err(BrowserAdmissionError::CsrfRequired);
        }
        return Ok(());
    }

    pub fn admit_session_set_cookie(
        &self,
        _adapter: HttpIngressAdapter,
        set_cookie: &str,
    ) -> Result<(), BrowserAdmissionError> {
        if !self.require_secure_session_cookies {
            return Ok(());
        }
        let attributes = set_cookie
            .split(';')
            .skip(1)
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>();
        let secure = attributes.iter().any(|value| value == "secure");
        let http_only = attributes.iter().any(|value| value == "httponly");
        let same_site = attributes
            .iter()
            .any(|value| matches!(value.as_str(), "samesite=lax" | "samesite=strict"));
        if !secure || !http_only || !same_site {
            return Err(BrowserAdmissionError::InsecureSessionCookie);
        }
        return Ok(());
    }
}

fn unsafe_method(method: &str) -> bool {
    return matches!(
        method.to_ascii_uppercase().as_str(),
        "POST" | "PUT" | "PATCH" | "DELETE"
    );
}

#[must_use]
pub fn safe_problem_headers(
    headers: impl IntoIterator<Item = (String, String)>,
) -> BTreeMap<String, String> {
    return headers
        .into_iter()
        .filter_map(|(name, value)| {
            let normalized = name.to_ascii_lowercase();
            let admitted = matches!(
                normalized.as_str(),
                "retry-after"
                    | "ratelimit-limit"
                    | "ratelimit-remaining"
                    | "ratelimit-reset"
                    | "x-request-id"
                    | "x-ores-request-id"
            ) && !value.is_empty()
                && value.len() <= 256
                && !value
                    .bytes()
                    .any(|byte| byte == 0 || byte == b'\r' || byte == b'\n');
            if admitted {
                return Some((normalized, value));
            }
            return None;
        })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> RequestBufferLimits {
        return RequestBufferLimits {
            max_header_count: 4,
            max_header_bytes: 64,
            max_body_bytes: 1024,
            max_upload_bytes: 512,
            max_decompressed_bytes: 2048,
            max_decompression_ratio: 8,
        };
    }

    #[test]
    fn oversized_heads_and_declared_bodies_fail_before_buffering() {
        let large = "b".repeat(80);
        assert_eq!(
            preflight_request_buffers(
                limits(),
                &[("x-a", "1"), ("x-b", large.as_str())],
                RequestBufferMetadata {
                    declared_body_bytes: None,
                    upload: false,
                },
            ),
            Err(BufferAdmissionError::HeaderValueExceeded)
        );
        assert_eq!(
            preflight_request_buffers(
                limits(),
                &[],
                RequestBufferMetadata {
                    declared_body_bytes: Some(513),
                    upload: true,
                },
            ),
            Err(BufferAdmissionError::UploadExceeded)
        );
        assert_eq!(
            preflight_request_buffers(
                limits(),
                &[],
                RequestBufferMetadata {
                    declared_body_bytes: Some(1025),
                    upload: false,
                },
            ),
            Err(BufferAdmissionError::BodyExceeded)
        );
    }

    #[test]
    fn decompression_is_bounded_incrementally_before_retaining_expansion() {
        let mut budget = DecompressionBudget::new(limits()).expect("valid limits");
        assert_eq!(budget.admit_chunk(100, 400), Ok(()));
        assert_eq!(
            budget.admit_chunk(1, 500),
            Err(BufferAdmissionError::DecompressionRatioExceeded)
        );

        let mut absolute = DecompressionBudget::new(limits()).expect("valid limits");
        assert_eq!(
            absolute.admit_chunk(300, 2049),
            Err(BufferAdmissionError::DecompressedExceeded)
        );
    }

    #[test]
    fn public_errors_are_adapter_equivalent_and_never_include_diagnostics() {
        let encoded = CERTIFIED_HTTP_INGRESS_ADAPTERS
            .iter()
            .map(|adapter| {
                return encode_public_problem(&public_problem(
                    *adapter,
                    500,
                    "internal_error",
                    Some("req-123"),
                    Some("postgres password=secret stack trace /srv/app.rs:99"),
                ));
            })
            .collect::<Vec<_>>();
        assert!(encoded.windows(2).all(|pair| pair[0] == pair[1]));
        let text = String::from_utf8(encoded[0].clone()).expect("problem JSON");
        assert!(!text.contains("password"));
        assert!(!text.contains("stack trace"));
        assert!(!text.contains("app.rs"));
    }

    #[test]
    fn forwarded_metadata_is_admitted_only_from_configured_ingress() {
        let policy = TrustedProxyPolicy {
            trusted_proxy_cidrs: vec!["10.0.0.0/8".to_owned()],
        };
        assert_eq!(
            policy.resolve(ForwardedRequest {
                socket_peer: "203.0.113.7".parse().unwrap(),
                socket_host: "internal:8080",
                socket_scheme: "http",
                forwarded_host: Some("api.example.com"),
                forwarded_scheme: Some("https"),
                forwarded_client: Some("198.51.100.9"),
            }),
            Err(ProxyAdmissionError::UntrustedForwardingPeer)
        );

        let trusted = policy
            .resolve(ForwardedRequest {
                socket_peer: "10.2.3.4".parse().unwrap(),
                socket_host: "internal:8080",
                socket_scheme: "http",
                forwarded_host: Some("api.example.com"),
                forwarded_scheme: Some("https"),
                forwarded_client: Some("198.51.100.9"),
            })
            .expect("trusted proxy metadata");
        assert_eq!(trusted.host, "api.example.com");
        assert_eq!(trusted.scheme, "https");
        assert_eq!(trusted.client_ip, "198.51.100.9".parse::<IpAddr>().unwrap());
        assert!(trusted.used_forwarded_metadata);
    }

    #[test]
    fn browser_security_is_identical_for_every_certified_ingress_adapter() {
        let policy = BrowserSecurityPolicy {
            allowed_origins: vec!["https://app.example.com".to_owned()],
            allow_credentials: true,
            require_csrf_for_cookie_mutations: true,
            require_secure_session_cookies: true,
        };
        for adapter in CERTIFIED_HTTP_INGRESS_ADAPTERS {
            assert_eq!(
                policy.admit_request(
                    *adapter,
                    BrowserRequest {
                        method: "POST",
                        origin: Some("https://app.example.com"),
                        has_session_cookie: true,
                        csrf_verified: false,
                    }
                ),
                Err(BrowserAdmissionError::CsrfRequired)
            );
            assert_eq!(
                policy.admit_request(
                    *adapter,
                    BrowserRequest {
                        method: "GET",
                        origin: Some("https://evil.example"),
                        has_session_cookie: false,
                        csrf_verified: false,
                    }
                ),
                Err(BrowserAdmissionError::OriginDenied)
            );
            assert_eq!(
                policy.admit_session_set_cookie(
                    *adapter,
                    "session=opaque; Path=/; Secure; HttpOnly; SameSite=Lax"
                ),
                Ok(())
            );
            assert_eq!(
                policy.admit_session_set_cookie(*adapter, "session=opaque; Path=/; HttpOnly"),
                Err(BrowserAdmissionError::InsecureSessionCookie)
            );
        }
    }
}

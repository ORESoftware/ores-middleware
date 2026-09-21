//! Provider-neutral host ABI for isolated middleware execution.
//!
//! The ABI deliberately contains no process, socket, filesystem, Axum, or edge
//! provider types. A local P2 process and an edge worker adapter can therefore
//! normalize their trusted host facts into the same request/finish contract.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{MiddlewareConfig, RequestMetadata, CONTRACT_VERSION};

pub const MIDDLEWARE_HOST_ABI_SCHEMA: &str = "ores.middleware.host/v1";
pub const MIDDLEWARE_HOST_ABI_VERSION: &str = "1";
pub const MAX_HOST_HEADER_COUNT: usize = 256;
pub const MAX_HOST_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_HOST_METHOD_BYTES: usize = 32;
pub const MAX_HOST_PATH_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiddlewareHostKind {
    LocalProcess,
    CloudflareWorker,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiddlewareHostOutcome {
    Completed,
    Disconnected,
    TimedOut,
    ChildFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MiddlewareHostDescriptor {
    pub schema: String,
    pub abi_version: String,
    pub middleware_contract_version: String,
    pub host_kind: MiddlewareHostKind,
    pub config_sha256: String,
}

impl MiddlewareHostDescriptor {
    pub fn for_config(
        host_kind: MiddlewareHostKind,
        config: &MiddlewareConfig,
    ) -> Result<Self, MiddlewareHostAbiError> {
        Ok(Self {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            abi_version: MIDDLEWARE_HOST_ABI_VERSION.to_owned(),
            middleware_contract_version: CONTRACT_VERSION.to_owned(),
            host_kind,
            config_sha256: middleware_config_sha256(config)?,
        })
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MiddlewareHostRequest {
    pub schema: String,
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    /// Remote address admitted by the host adapter as a trusted transport fact.
    /// Never populate this by copying an arbitrary forwarded request header.
    pub trusted_remote_ip: Option<String>,
    pub content_length: Option<u64>,
    /// TLS/security state admitted by the host adapter, never inferred from a
    /// caller-controlled forwarding header.
    pub trusted_transport_secure: bool,
}

impl fmt::Debug for MiddlewareHostRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MiddlewareHostRequest")
            .field("schema", &self.schema)
            .field("method", &self.method)
            .field("path_bytes", &self.path.len())
            .field("header_count", &self.headers.len())
            .field("trusted_remote_ip_present", &self.trusted_remote_ip.is_some())
            .field("content_length", &self.content_length)
            .field("trusted_transport_secure", &self.trusted_transport_secure)
            .finish()
    }
}

impl MiddlewareHostRequest {
    #[must_use]
    pub fn new(method: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            method: method.into(),
            path: path.into(),
            headers: BTreeMap::new(),
            trusted_remote_ip: None,
            content_length: None,
            trusted_transport_secure: false,
        }
    }

    pub fn validate(&self) -> Result<(), MiddlewareHostAbiError> {
        validate_schema(&self.schema)?;
        if self.method.is_empty() || self.method.len() > MAX_HOST_METHOD_BYTES {
            return Err(MiddlewareHostAbiError::new(
                "invalid_method",
                "middleware host method is empty or exceeds the bounded ABI limit",
            ));
        }
        if !self.method.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        }) {
            return Err(MiddlewareHostAbiError::new(
                "invalid_method",
                "middleware host method must use normalized uppercase ASCII token bytes",
            ));
        }
        if self.path.is_empty()
            || self.path.len() > MAX_HOST_PATH_BYTES
            || !self.path.starts_with('/')
            || self.path.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(MiddlewareHostAbiError::new(
                "invalid_path",
                "middleware host path is not a bounded normalized absolute path",
            ));
        }
        if self.headers.len() > MAX_HOST_HEADER_COUNT {
            return Err(MiddlewareHostAbiError::new(
                "too_many_headers",
                "middleware host request exceeds the header-count limit",
            ));
        }
        let mut total = 0usize;
        for (name, value) in &self.headers {
            if !valid_header_name(name) {
                return Err(MiddlewareHostAbiError::new(
                    "invalid_header_name",
                    "middleware host headers must use canonical lowercase ASCII names",
                ));
            }
            if value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0)) {
                return Err(MiddlewareHostAbiError::new(
                    "invalid_header_value",
                    "middleware host header values may not contain framing bytes",
                ));
            }
            total = total
                .checked_add(name.len())
                .and_then(|value_bytes| value_bytes.checked_add(value.len()))
                .ok_or_else(|| {
                    MiddlewareHostAbiError::new(
                        "headers_too_large",
                        "middleware host header size overflowed",
                    )
                })?;
        }
        if total > MAX_HOST_HEADER_BYTES {
            return Err(MiddlewareHostAbiError::new(
                "headers_too_large",
                "middleware host request exceeds the aggregate header-byte limit",
            ));
        }
        Ok(())
    }

    pub fn into_request_metadata(self) -> Result<RequestMetadata, MiddlewareHostAbiError> {
        self.validate()?;
        Ok(RequestMetadata {
            method: self.method,
            path: self.path,
            headers: self.headers,
            remote_ip: self.trusted_remote_ip,
            content_length: self.content_length,
            transport_secure: self.trusted_transport_secure,
        })
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum MiddlewareHostBeginResult {
    Permit {
        schema: String,
        session_id: String,
        request_id: String,
        trace_id: String,
        response_headers: BTreeMap<String, String>,
    },
    Reject {
        schema: String,
        status: u16,
        code: String,
        message: String,
        headers: BTreeMap<String, String>,
    },
}

impl fmt::Debug for MiddlewareHostBeginResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Permit {
                schema,
                session_id,
                request_id,
                trace_id,
                response_headers,
            } => formatter
                .debug_struct("MiddlewareHostBeginResult::Permit")
                .field("schema", schema)
                .field("session_id", session_id)
                .field("request_id", request_id)
                .field("trace_id", trace_id)
                .field("response_header_count", &response_headers.len())
                .finish(),
            Self::Reject {
                schema,
                status,
                code,
                message: _,
                headers,
            } => formatter
                .debug_struct("MiddlewareHostBeginResult::Reject")
                .field("schema", schema)
                .field("status", status)
                .field("code", code)
                .field("header_count", &headers.len())
                .finish(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MiddlewareHostFinishRequest {
    pub schema: String,
    pub session_id: String,
    pub status: u16,
    pub response_bytes: Option<u64>,
    pub outcome: MiddlewareHostOutcome,
}

impl MiddlewareHostFinishRequest {
    pub fn validate(&self) -> Result<(), MiddlewareHostAbiError> {
        validate_schema(&self.schema)?;
        if self.session_id.is_empty() || self.session_id.len() > 128 {
            return Err(MiddlewareHostAbiError::new(
                "invalid_session_id",
                "middleware host session id is empty or too long",
            ));
        }
        if !(100..=599).contains(&self.status) {
            return Err(MiddlewareHostAbiError::new(
                "invalid_status",
                "middleware host final status must be an HTTP status code",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MiddlewareHostFinishResult {
    pub schema: String,
    pub outcome: MiddlewareHostOutcome,
    pub response_headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MiddlewareHostAbiError {
    pub code: &'static str,
    pub message: String,
}

impl MiddlewareHostAbiError {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for MiddlewareHostAbiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for MiddlewareHostAbiError {}

pub fn middleware_config_sha256(
    config: &MiddlewareConfig,
) -> Result<String, MiddlewareHostAbiError> {
    let encoded = serde_json::to_vec(config).map_err(|_| {
        MiddlewareHostAbiError::new(
            "config_identity_failed",
            "middleware config could not be serialized for host identity",
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn validate_schema(schema: &str) -> Result<(), MiddlewareHostAbiError> {
    if schema == MIDDLEWARE_HOST_ABI_SCHEMA {
        Ok(())
    } else {
        Err(MiddlewareHostAbiError::new(
            "unsupported_host_abi",
            "middleware host frame uses an unsupported ABI schema",
        ))
    }
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_config;

    #[test]
    fn host_request_debug_never_prints_header_or_remote_ip_values() {
        let mut request = MiddlewareHostRequest::new("GET", "/secret?token=redacted-by-host");
        request.headers.insert("authorization".into(), "Bearer top-secret".into());
        request.trusted_remote_ip = Some("203.0.113.19".into());
        let debug = format!("{request:?}");
        assert!(!debug.contains("top-secret"));
        assert!(!debug.contains("203.0.113.19"));
        assert!(!debug.contains("token=redacted-by-host"));
    }

    #[test]
    fn canonical_lowercase_headers_are_required() {
        let mut request = MiddlewareHostRequest::new("GET", "/");
        request.headers.insert("Authorization".into(), "redacted".into());
        assert_eq!(request.validate().unwrap_err().code, "invalid_header_name");
    }

    #[test]
    fn config_identity_is_deterministic_and_semantic() {
        let first = default_config("host-abi");
        let mut second = first.clone();
        assert_eq!(
            middleware_config_sha256(&first).unwrap(),
            middleware_config_sha256(&second).unwrap()
        );
        second.settings.max_body_bytes += 1;
        assert_ne!(
            middleware_config_sha256(&first).unwrap(),
            middleware_config_sha256(&second).unwrap()
        );
    }

    #[test]
    fn descriptor_separates_host_adapter_from_config_identity() {
        let config = default_config("host-abi");
        let local = MiddlewareHostDescriptor::for_config(MiddlewareHostKind::LocalProcess, &config)
            .unwrap();
        let edge = MiddlewareHostDescriptor::for_config(MiddlewareHostKind::CloudflareWorker, &config)
            .unwrap();
        assert_eq!(local.config_sha256, edge.config_sha256);
        assert_ne!(local.host_kind, edge.host_kind);
    }
}

//! Provider-neutral host ABI for isolated middleware execution.
//!
//! The ABI deliberately contains no process, socket, filesystem, Axum, or edge
//! provider types. A local P2 process and an edge worker adapter can therefore
//! normalize their trusted host facts into the same request/lifecycle contract.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{CONTRACT_VERSION, MiddlewareConfig, RequestMetadata};

pub const MIDDLEWARE_HOST_ABI_SCHEMA: &str = "ores.middleware.host/v1";
pub const MIDDLEWARE_HOST_ABI_VERSION: &str = "1";
pub const MAX_HOST_ADAPTER_ID_BYTES: usize = 128;
pub const MAX_HOST_HEADER_COUNT: usize = 256;
pub const MAX_HOST_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_HOST_METHOD_BYTES: usize = 32;
pub const MAX_HOST_PATH_BYTES: usize = 8 * 1024;
pub const MAX_HOST_SESSION_ID_BYTES: usize = 128;

/// Compatibility selector retained for callers compiled against the first Rust
/// host-ABI surface. Serialized host descriptors use adapter id + execution
/// model + capabilities, matching the independent TypeSpec/JSON authorities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MiddlewareHostKind {
    LocalProcess,
    CloudflareWorker,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiddlewareHostExecutionModel {
    LocalProcess,
    FetchHandler,
    EventHooks,
    LambdaEvents,
    WasiHttp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MiddlewareHostCapabilities {
    pub streaming_response: bool,
    pub response_head_available_observation: bool,
    pub response_head_commit_observation: bool,
    pub body_stream_completion_observation: bool,
    pub transport_completion_observation: bool,
    pub client_disconnect_observation: bool,
    pub background_wait_until: bool,
    pub wasm_module: bool,
}

impl MiddlewareHostCapabilities {
    #[must_use]
    pub const fn local_process() -> Self {
        Self {
            streaming_response: true,
            response_head_available_observation: true,
            response_head_commit_observation: true,
            body_stream_completion_observation: true,
            transport_completion_observation: true,
            client_disconnect_observation: true,
            background_wait_until: false,
            wasm_module: false,
        }
    }

    #[must_use]
    pub const fn fetch_handler() -> Self {
        Self {
            streaming_response: true,
            response_head_available_observation: true,
            response_head_commit_observation: false,
            body_stream_completion_observation: true,
            transport_completion_observation: false,
            client_disconnect_observation: false,
            background_wait_until: true,
            wasm_module: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiddlewareHostResponseHeadPhase {
    Available,
    Committed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiddlewareHostCompletionBoundary {
    BodyStream,
    Transport,
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
    pub adapter_id: String,
    pub execution_model: MiddlewareHostExecutionModel,
    pub capabilities: MiddlewareHostCapabilities,
    pub config_sha256: String,
}

impl MiddlewareHostDescriptor {
    pub fn for_adapter_config(
        adapter_id: impl Into<String>,
        execution_model: MiddlewareHostExecutionModel,
        capabilities: MiddlewareHostCapabilities,
        config: &MiddlewareConfig,
    ) -> Result<Self, MiddlewareHostAbiError> {
        let adapter_id = adapter_id.into();
        validate_adapter_id(&adapter_id)?;
        Ok(Self {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            abi_version: MIDDLEWARE_HOST_ABI_VERSION.to_owned(),
            middleware_contract_version: CONTRACT_VERSION.to_owned(),
            adapter_id,
            execution_model,
            capabilities,
            config_sha256: middleware_config_sha256(config)?,
        })
    }

    /// Compatibility constructor for the original Rust-only host kind enum.
    /// New adapters should use [`Self::for_adapter_config`].
    pub fn for_config(
        host_kind: MiddlewareHostKind,
        config: &MiddlewareConfig,
    ) -> Result<Self, MiddlewareHostAbiError> {
        match host_kind {
            MiddlewareHostKind::LocalProcess => Self::for_adapter_config(
                "ores.local-process",
                MiddlewareHostExecutionModel::LocalProcess,
                MiddlewareHostCapabilities::local_process(),
                config,
            ),
            MiddlewareHostKind::CloudflareWorker => Self::for_adapter_config(
                "cloudflare.worker",
                MiddlewareHostExecutionModel::FetchHandler,
                MiddlewareHostCapabilities::fetch_handler(),
                config,
            ),
        }
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
            .field(
                "trusted_remote_ip_present",
                &self.trusted_remote_ip.is_some(),
            )
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
        validate_headers(&self.headers)?;
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
#[serde(
    tag = "decision",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
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
pub struct MiddlewareHostResponseHeadRequest {
    pub schema: String,
    pub session_id: String,
    pub status: u16,
    pub phase: MiddlewareHostResponseHeadPhase,
}

impl MiddlewareHostResponseHeadRequest {
    pub fn validate(&self) -> Result<(), MiddlewareHostAbiError> {
        validate_schema(&self.schema)?;
        validate_session_id(&self.session_id)?;
        validate_status(self.status)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MiddlewareHostResponseHeadResult {
    pub schema: String,
    pub status: u16,
    pub phase: MiddlewareHostResponseHeadPhase,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MiddlewareHostFinishRequest {
    pub schema: String,
    pub session_id: String,
    pub status: u16,
    pub response_bytes: Option<u64>,
    pub outcome: MiddlewareHostOutcome,
    pub completion_boundary: MiddlewareHostCompletionBoundary,
}

impl MiddlewareHostFinishRequest {
    pub fn validate(&self) -> Result<(), MiddlewareHostAbiError> {
        validate_schema(&self.schema)?;
        validate_session_id(&self.session_id)?;
        validate_status(self.status)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MiddlewareHostFinishResult {
    pub schema: String,
    pub outcome: MiddlewareHostOutcome,
    pub completion_boundary: MiddlewareHostCompletionBoundary,
    pub status: u16,
    pub time_to_response_head_available_ms: Option<u64>,
    pub time_to_response_head_committed_ms: Option<u64>,
    pub total_duration_ms: u64,
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

fn validate_adapter_id(adapter_id: &str) -> Result<(), MiddlewareHostAbiError> {
    if adapter_id.is_empty()
        || adapter_id.len() > MAX_HOST_ADAPTER_ID_BYTES
        || !adapter_id.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_lowercase() || byte.is_ascii_digit()
            } else {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'/' | b'-')
            }
        })
    {
        return Err(MiddlewareHostAbiError::new(
            "invalid_adapter_id",
            "middleware host adapter id violates the canonical ABI pattern",
        ));
    }
    Ok(())
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

fn validate_session_id(session_id: &str) -> Result<(), MiddlewareHostAbiError> {
    if session_id.is_empty() || session_id.len() > MAX_HOST_SESSION_ID_BYTES {
        Err(MiddlewareHostAbiError::new(
            "invalid_session_id",
            "middleware host session id is empty or too long",
        ))
    } else {
        Ok(())
    }
}

fn validate_status(status: u16) -> Result<(), MiddlewareHostAbiError> {
    if (100..=599).contains(&status) {
        Ok(())
    } else {
        Err(MiddlewareHostAbiError::new(
            "invalid_status",
            "middleware host status must be an HTTP status code",
        ))
    }
}

fn validate_headers(headers: &BTreeMap<String, String>) -> Result<(), MiddlewareHostAbiError> {
    if headers.len() > MAX_HOST_HEADER_COUNT {
        return Err(MiddlewareHostAbiError::new(
            "too_many_headers",
            "middleware host request exceeds the header-count limit",
        ));
    }
    let mut total = 0usize;
    for (name, value) in headers {
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

fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_config;

    #[test]
    fn host_request_debug_never_prints_header_or_remote_ip_values() {
        let mut request = MiddlewareHostRequest::new("GET", "/secret?token=redacted-by-host");
        request
            .headers
            .insert("authorization".into(), "Bearer top-secret".into());
        request.trusted_remote_ip = Some("203.0.113.19".into());
        let debug = format!("{request:?}");
        assert!(!debug.contains("top-secret"));
        assert!(!debug.contains("203.0.113.19"));
        assert!(!debug.contains("token=redacted-by-host"));
    }

    #[test]
    fn canonical_lowercase_headers_are_required() {
        let mut request = MiddlewareHostRequest::new("GET", "/");
        request
            .headers
            .insert("Authorization".into(), "redacted".into());
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
    fn descriptor_uses_capability_driven_host_identity() {
        let config = default_config("host-abi");
        let local = MiddlewareHostDescriptor::for_config(MiddlewareHostKind::LocalProcess, &config)
            .unwrap();
        let edge =
            MiddlewareHostDescriptor::for_config(MiddlewareHostKind::CloudflareWorker, &config)
                .unwrap();
        assert_eq!(local.config_sha256, edge.config_sha256);
        assert_ne!(local.adapter_id, edge.adapter_id);
        assert_ne!(local.execution_model, edge.execution_model);
        assert!(local.capabilities.response_head_commit_observation);
        assert!(!edge.capabilities.response_head_commit_observation);
    }

    #[test]
    fn invalid_adapter_ids_fail_closed() {
        let config = default_config("host-abi");
        let error = MiddlewareHostDescriptor::for_adapter_config(
            "Cloudflare Worker",
            MiddlewareHostExecutionModel::FetchHandler,
            MiddlewareHostCapabilities::fetch_handler(),
            &config,
        )
        .unwrap_err();
        assert_eq!(error.code, "invalid_adapter_id");
    }

    #[test]
    fn begin_result_uses_authored_camel_case_wire_names() {
        let value = serde_json::to_value(MiddlewareHostBeginResult::Permit {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            session_id: "session-1".to_owned(),
            request_id: "request-1".to_owned(),
            trace_id: "trace-1".to_owned(),
            response_headers: BTreeMap::new(),
        })
        .expect("serialize begin result");
        assert_eq!(value["decision"], "permit");
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["requestId"], "request-1");
        assert_eq!(value["traceId"], "trace-1");
        assert!(value.get("responseHeaders").is_some());
        assert!(value.get("session_id").is_none());
    }
}

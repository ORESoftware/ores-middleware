use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
};

use crate::{IntegrationError, RequestContext, RequestMetadata};

/// Boxed future used by the import-light callback adapters.
pub type EdgeCallbackFuture<'a> = Pin<
    Box<dyn Future<Output = Result<EdgeMinimalDecision, IntegrationError>> + Send + 'a>,
>;

/// Host-provided outbound Fetch-style client.
///
/// P2 middleware receives this capability by injection rather than importing a
/// provider SDK or opening sockets directly. Edge adapters can implement it with
/// the provider's native `fetch`; local adapters can implement it with an HTTP
/// client while preserving the same middleware callback surface.
pub trait EdgeFetchClient: Send + Sync {
    fn fetch<'a>(
        &'a self,
        request: EdgeFetchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<EdgeFetchResponse, IntegrationError>> + Send + 'a>>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EdgeFetchRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EdgeFetchResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// Minimal structured logger injected by the host.
///
/// Consumers do not import a tracing/logging backend in portable P2 code. The
/// host decides whether this becomes tracing, console output, OTEL events, or a
/// provider-native log sink.
pub trait EdgeLogger: Send + Sync {
    fn event(&self, level: EdgeLogLevel, name: &str, fields: &BTreeMap<String, String>);
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum EdgeLogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Default)]
pub struct NoopEdgeLogger;

impl EdgeLogger for NoopEdgeLogger {
    fn event(&self, _level: EdgeLogLevel, _name: &str, _fields: &BTreeMap<String, String>) {}
}

/// The complete dependency surface admitted for `edge_minimal` callbacks.
///
/// Keep this intentionally small. Adding a field here expands the portable P2
/// contract and therefore requires capability/conformance review across every
/// supported edge adapter.
pub struct EdgeMinimalDependencies<'a> {
    pub fetch: &'a dyn EdgeFetchClient,
    pub logger: &'a dyn EdgeLogger,
}

/// Restricted mutable request view for P2 middleware.
///
/// There is intentionally no request-body, filesystem, database, process,
/// listener, raw socket, or raw-fd accessor.
pub struct EdgeMinimalRequest<'a> {
    inner: &'a mut RequestMetadata,
}

impl<'a> EdgeMinimalRequest<'a> {
    #[must_use]
    pub fn new(inner: &'a mut RequestMetadata) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn method(&self) -> &str {
        &self.inner.method
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.inner.path
    }

    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.inner.headers.get(name).map(String::as_str)
    }

    pub fn set_header(
        &mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), IntegrationError> {
        let name = name.into();
        let value = value.into();
        if !valid_portable_header_name(&name) {
            return Err(IntegrationError {
                code: "edge_header_name_invalid",
                message: "edge middleware request headers must use canonical lowercase ASCII names"
                    .into(),
            });
        }
        if value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0)) {
            return Err(IntegrationError {
                code: "edge_header_value_invalid",
                message: "edge middleware request header values may not contain framing bytes"
                    .into(),
            });
        }
        self.inner.headers.insert(name, value);
        Ok(())
    }

    pub fn remove_header(&mut self, name: &str) -> Option<String> {
        self.inner.headers.remove(name)
    }
}

/// Everything an import-light `edge_minimal` callback is allowed to receive.
pub struct EdgeMinimalInvocation<'a> {
    pub request: EdgeMinimalRequest<'a>,
    pub context: &'a RequestContext,
    pub deps: EdgeMinimalDependencies<'a>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum EdgeMinimalDecision {
    Continue,
    Reject {
        status: u16,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    },
}

/// Extensible middleware provider implemented by consumers or generated wrappers.
///
/// The blanket implementation below means consumers can provide a callback
/// directly instead of defining a bespoke middleware type.
pub trait EdgeMinimalMiddleware: Send + Sync {
    fn handle<'a>(&'a self, invocation: EdgeMinimalInvocation<'a>) -> EdgeCallbackFuture<'a>;
}

impl<F> EdgeMinimalMiddleware for F
where
    F: Send + Sync + for<'a> Fn(EdgeMinimalInvocation<'a>) -> EdgeCallbackFuture<'a>,
{
    fn handle<'a>(&'a self, invocation: EdgeMinimalInvocation<'a>) -> EdgeCallbackFuture<'a> {
        (self)(invocation)
    }
}

fn valid_portable_header_name(name: &str) -> bool {
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

    fn request() -> RequestMetadata {
        RequestMetadata {
            method: "GET".into(),
            path: "/private".into(),
            headers: BTreeMap::new(),
            remote_ip: None,
            content_length: None,
            transport_secure: true,
        }
    }

    #[test]
    fn restricted_request_can_only_mutate_canonical_headers() {
        let mut metadata = request();
        let mut request = EdgeMinimalRequest::new(&mut metadata);
        request
            .set_header("x-ores-user-id", "user-123")
            .expect("canonical header");
        assert_eq!(request.header("x-ores-user-id"), Some("user-123"));
        assert_eq!(
            request.set_header("Authorization", "secret").unwrap_err().code,
            "edge_header_name_invalid"
        );
    }

    #[test]
    fn restricted_request_rejects_header_framing_bytes() {
        let mut metadata = request();
        let mut request = EdgeMinimalRequest::new(&mut metadata);
        assert_eq!(
            request
                .set_header("x-test", "ok\r\nset-cookie: nope")
                .unwrap_err()
                .code,
            "edge_header_value_invalid"
        );
    }
}

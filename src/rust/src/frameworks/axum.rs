use std::{
    collections::BTreeMap,
    convert::Infallible,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, DefaultBodyLimit, Request, State},
    http::{HeaderName, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use next_loggers::{Event, JsonObject, Logger, Value};
use serde_json::json;
#[cfg(feature = "compression")]
use tower_http::compression::CompressionLayer;

use crate::{
    BootstrapError, MiddlewareError, MiddlewareStack, admit_server_stack,
    integrations::{RequestMetadata, TransportSecurity},
    operation::{
        OperationDescriptor, OperationFailureKind, OperationOutcome, OperationScope,
        OperationTransport, run_operation_boundary_with_timeout,
    },
    otel::RequestLogger,
    stack_from_env,
};

#[derive(Clone)]
struct DispatchState {
    stack: Arc<MiddlewareStack>,
    logger: Option<Logger>,
}

pub fn install_from_env(
    router: Router,
    service_name: impl Into<String>,
) -> Result<Router, BootstrapError> {
    let stack = Arc::new(stack_from_env(service_name)?);
    Ok(install(router, stack))
}

/// Install from the existing environment contract only after the executable's embedded
/// `.ores-mw.toml` selects the expected enabled server stack target.
///
/// This transition API makes the repository manifest runtime-active without replacing
/// `.cli-flags.toml` or the existing environment bootstrap. The full middleware stack JSON remains
/// governed independently; callers pass its repository-relative path so manifest/path drift fails
/// before a stack is installed.
pub fn install_from_env_with_manifest(
    router: Router,
    service_name: impl Into<String>,
    manifest_source: &str,
    target_name: Option<&str>,
    expected_stack_config: &str,
) -> Result<Router, BootstrapError> {
    admit_server_stack(manifest_source, target_name, expected_stack_config).map_err(|error| {
        BootstrapError {
            variable: None,
            code: error.code(),
            message: ".ores-mw.toml runtime target admission failed".into(),
        }
    })?;
    install_from_env(router, service_name)
}

pub fn install_from_env_with_ores_logger(
    router: Router,
    service_name: impl Into<String>,
    logger: Logger,
) -> Result<Router, BootstrapError> {
    let stack = Arc::new(stack_from_env(service_name)?);
    Ok(install_with_ores_logger(router, stack, logger))
}

pub fn install(router: Router, stack: Arc<MiddlewareStack>) -> Router {
    install_with_state(
        router,
        DispatchState {
            stack,
            logger: None,
        },
    )
}

/// Installs the portable stack plus an ores-otel request logger. Handlers may
/// extract [`RequestLogger`] from request extensions, while any file/module
/// logger can call `info_context` or `warn_context` inside the same task.
pub fn install_with_ores_logger(
    router: Router,
    stack: Arc<MiddlewareStack>,
    logger: Logger,
) -> Router {
    install_with_state(
        router,
        DispatchState {
            stack,
            logger: Some(logger),
        },
    )
}

fn install_with_state(router: Router, state: DispatchState) -> Router {
    let max_body_bytes = state.stack.config().settings.max_body_bytes;
    #[cfg(feature = "compression")]
    let compression_enabled = state.stack.config().settings.compression.enabled;

    let router = router
        .layer(middleware::from_fn_with_state(state, dispatch))
        .layer(DefaultBodyLimit::max(max_body_bytes));

    #[cfg(feature = "compression")]
    if compression_enabled {
        return router.layer(CompressionLayer::new());
    }

    router
}

async fn dispatch(
    State(state): State<DispatchState>,
    mut request: Request,
    next: Next,
) -> Response {
    let metadata = request_metadata(&request);
    let active = match state.stack.begin(metadata.clone()).await {
        Ok(active) => active,
        Err(error) => return problem(error),
    };
    request.extensions_mut().insert(active.context.clone());

    let request_logger = state
        .logger
        .as_ref()
        .map(|logger| RequestLogger::new(logger.clone(), &active.context));
    if let Some(logger) = &request_logger {
        request.extensions_mut().insert(logger.clone());
        emit_request_log(
            logger
                .info(vec![Value::String("request handler started".into())])
                .add_fields(request_log_fields(&metadata)),
            "started",
        );
    }

    let context = active.context.clone();
    let timeout = Duration::from_millis(state.stack.config().settings.timeout_ms);
    let started = Instant::now();
    let outcome = run_operation_boundary_with_timeout(
        context,
        OperationDescriptor {
            transport: OperationTransport::Http,
            scope: OperationScope::Request,
            name: "middleware.handler".into(),
        },
        timeout,
        async move { Ok::<_, Infallible>(next.run(request).await) },
    )
    .await;

    let mut response = match outcome {
        OperationOutcome::Completed(response) => {
            if let Some(logger) = &request_logger {
                emit_request_log(
                    logger
                        .info(vec![Value::String("request handler completed".into())])
                        .add_fields(request_outcome_fields(
                            &metadata,
                            "completed",
                            started.elapsed(),
                            Some(response.status().as_u16()),
                        )),
                    "completed",
                );
            }
            response
        }
        OperationOutcome::Failed(failure) => {
            let (outcome, status, code, detail, log_message) = match failure.kind {
                OperationFailureKind::DeadlineExceeded => (
                    "timeout",
                    504,
                    "deadline_exceeded",
                    "request deadline exceeded",
                    "request handler timed out",
                ),
                OperationFailureKind::Cancelled => (
                    "cancelled",
                    499,
                    "request_cancelled",
                    "request was cancelled",
                    "request handler cancelled",
                ),
                OperationFailureKind::Error => (
                    "error",
                    500,
                    "internal_error",
                    "request handler failed",
                    "request handler failed",
                ),
                OperationFailureKind::Panic => (
                    "panic",
                    500,
                    "internal_error",
                    "request handler failed",
                    "request handler panicked",
                ),
            };
            if let Some(logger) = &request_logger {
                emit_request_log(
                    logger
                        .error(vec![Value::String(log_message.into())])
                        .add_fields(request_outcome_fields(
                            &metadata,
                            outcome,
                            started.elapsed(),
                            Some(status),
                        )),
                    outcome,
                );
            }
            problem(MiddlewareError::new(status, code, detail))
        }
    };
    let headers = state
        .stack
        .finish(active, response.status().as_u16(), None)
        .await;
    apply_finish_headers(&mut response, headers);
    response
        .headers_mut()
        .append("vary", HeaderValue::from_static("accept, accept-encoding"));
    response
}

fn apply_finish_headers(
    response: &mut Response,
    headers: impl IntoIterator<Item = (String, String)>,
) {
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            // A route-specific CSP can add stronger representation constraints such as `sandbox`.
            // Preserve both policies instead of replacing the handler policy with the middleware
            // default: user agents enforce multiple CSP fields cumulatively, so appending cannot
            // weaken the middleware baseline while retaining route-level restrictions.
            if name.as_str() == "content-security-policy"
                && response.headers().contains_key(&name)
            {
                response.headers_mut().append(name, value);
            } else {
                response.headers_mut().insert(name, value);
            }
        }
    }
}

fn emit_request_log(event: Event, phase: &'static str) {
    if let Err(error) = event.send() {
        tracing::warn!(phase, error = %error, "ores request log delivery failed");
    }
}

fn request_log_fields(metadata: &RequestMetadata) -> JsonObject {
    JsonObject::from_iter([
        (
            "http.request.method".into(),
            Value::String(metadata.method.clone()),
        ),
        ("url.path".into(), Value::String(metadata.path.clone())),
    ])
}

fn request_outcome_fields(
    metadata: &RequestMetadata,
    outcome: &str,
    duration: Duration,
    status: Option<u16>,
) -> JsonObject {
    let mut fields = request_log_fields(metadata);
    fields.insert("request.outcome".into(), Value::String(outcome.into()));
    fields.insert(
        "request.duration_ms".into(),
        Value::from(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)),
    );
    if let Some(status) = status {
        fields.insert("http.response.status_code".into(), Value::from(status));
    }
    fields
}

fn request_metadata(request: &Request) -> RequestMetadata {
    let headers = request
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    let remote_ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|value| value.0.ip().to_string());
    let content_length = request
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok());
    let transport_secure = request.uri().scheme_str() == Some("https")
        || request
            .extensions()
            .get::<TransportSecurity>()
            .is_some_and(|value| value.secure);
    RequestMetadata {
        method: request.method().to_string(),
        path: request.uri().path().to_owned(),
        headers,
        remote_ip,
        content_length,
        transport_secure,
    }
}

fn problem(error: MiddlewareError) -> Response {
    let MiddlewareError {
        status,
        code,
        message,
        headers,
    } = error;
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = (
        status,
        Json(json!({
            "type": format!("urn:ores:middleware:{code}"),
            "title": code,
            "status": status.as_u16(),
            "detail": message
        })),
    )
        .into_response();

    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            response.headers_mut().insert(name, value);
        }
    }
    response
}

pub type AxumBody = Body;

#[cfg(test)]
mod runtime_manifest_tests {
    use super::*;

    const MANIFEST: &str = r#"
schema_version = 1
repository_mode = "server-only"
default_target = "api"

[[targets]]
name = "api"
role = "server"
roots = ["src"]
middleware = "stack"
stack_config = "config/middleware.json"
"#;

    #[test]
    fn manifest_drift_blocks_install_before_environment_bootstrap() {
        let error = install_from_env_with_manifest(
            Router::new(),
            "fixture",
            MANIFEST,
            None,
            "config/other.json",
        )
        .expect_err("stack path drift must fail closed");
        assert_eq!(error.code, "runtime_stack_config_mismatch");
        assert!(error.variable.is_none());
        assert!(!error.message.contains("config/other.json"));
    }
}

#[cfg(test)]
mod response_header_precedence_tests {
    use super::*;

    #[test]
    fn route_specific_csp_is_composed_with_middleware_policy() {
        let mut response = Response::new(Body::empty());
        response.headers_mut().insert(
            "content-security-policy",
            HeaderValue::from_static("sandbox"),
        );

        apply_finish_headers(
            &mut response,
            [("content-security-policy".to_owned(),
              "default-src 'self'; frame-ancestors 'none'".to_owned())],
        );

        let policies = response
            .headers()
            .get_all("content-security-policy")
            .iter()
            .map(|value| value.to_str().expect("CSP header must be ASCII"))
            .collect::<Vec<_>>();
        assert_eq!(policies.len(), 2);
        assert_eq!(policies[0], "sandbox");
        assert_eq!(policies[1], "default-src 'self'; frame-ancestors 'none'");
    }

    #[test]
    fn middleware_csp_is_inserted_when_route_has_none() {
        let mut response = Response::new(Body::empty());
        apply_finish_headers(
            &mut response,
            [("content-security-policy".to_owned(),
              "default-src 'self'; frame-ancestors 'none'".to_owned())],
        );

        assert_eq!(
            response
                .headers()
                .get("content-security-policy")
                .and_then(|value| value.to_str().ok()),
            Some("default-src 'self'; frame-ancestors 'none'")
        );
    }
}

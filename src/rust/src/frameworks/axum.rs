use std::{
    collections::BTreeMap,
    net::SocketAddr,
    panic::AssertUnwindSafe,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::{ConnectInfo, DefaultBodyLimit, Request, State},
    http::{HeaderName, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Json, Router,
};
use futures_util::FutureExt;
use next_loggers::{Event, JsonObject, Logger, Value};
use serde_json::json;
#[cfg(feature = "compression")]
use tower_http::compression::CompressionLayer;

use crate::{
    context::run_with_context,
    integrations::{RequestMetadata, TransportSecurity},
    otel::{run_with_ores_log_context, RequestLogger},
    stack_from_env, BootstrapError, MiddlewareError, MiddlewareStack,
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
    let context_for_logs = context.clone();
    let ores_enabled = request_logger.is_some();
    let timeout = Duration::from_millis(state.stack.config().settings.timeout_ms);
    let started = Instant::now();
    let future = run_with_context(context, async move {
        let handler = AssertUnwindSafe(next.run(request)).catch_unwind();
        if ores_enabled {
            run_with_ores_log_context(&context_for_logs, handler).await
        } else {
            handler.await
        }
    });

    let mut response = match tokio::time::timeout(timeout, future).await {
        Err(_) => {
            if let Some(logger) = &request_logger {
                emit_request_log(
                    logger
                        .error(vec![Value::String("request handler timed out".into())])
                        .add_fields(request_outcome_fields(
                            &metadata,
                            "timeout",
                            started.elapsed(),
                            Some(504),
                        )),
                    "timeout",
                );
            }
            problem(MiddlewareError::new(
                504,
                "deadline_exceeded",
                "request deadline exceeded",
            ))
        }
        Ok(Err(_)) => {
            if let Some(logger) = &request_logger {
                emit_request_log(
                    logger
                        .error(vec![Value::String("request handler panicked".into())])
                        .add_fields(request_outcome_fields(
                            &metadata,
                            "panic",
                            started.elapsed(),
                            Some(500),
                        )),
                    "panic",
                );
            }
            problem(MiddlewareError::new(
                500,
                "internal_error",
                "request handler failed",
            ))
        }
        Ok(Ok(response)) => {
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
    };
    let headers = state
        .stack
        .finish(active, response.status().as_u16(), None)
        .await;
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            response.headers_mut().insert(name, value);
        }
    }
    response
        .headers_mut()
        .append("vary", HeaderValue::from_static("accept, accept-encoding"));
    response
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

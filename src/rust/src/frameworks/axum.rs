use std::{
    collections::BTreeMap,
    net::SocketAddr,
    panic::AssertUnwindSafe,
    sync::Arc,
    time::Duration,
};

use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{HeaderName, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Json, Router,
};
use futures_util::FutureExt;
use serde_json::json;
use tower_http::{compression::CompressionLayer, limit::RequestBodyLimitLayer};

use crate::{
    context::run_with_context,
    integrations::{RequestMetadata, TransportSecurity},
    stack_from_env, BootstrapError, MiddlewareError, MiddlewareStack,
};

pub fn install_from_env(
    router: Router,
    service_name: impl Into<String>,
) -> Result<Router, BootstrapError> {
    let stack = Arc::new(stack_from_env(service_name)?);
    Ok(install(router, stack))
}

pub fn install(router: Router, stack: Arc<MiddlewareStack>) -> Router {
    let compression = stack.config().settings.compression.enabled;
    let max_body_bytes = stack.config().settings.max_body_bytes;
    let router = router
        .layer(middleware::from_fn_with_state(stack, dispatch))
        .layer(RequestBodyLimitLayer::new(max_body_bytes));
    if compression {
        router.layer(CompressionLayer::new())
    } else {
        router
    }
}

async fn dispatch(
    State(stack): State<Arc<MiddlewareStack>>,
    mut request: Request,
    next: Next,
) -> Response {
    let metadata = request_metadata(&request);
    let active = match stack.begin(metadata).await {
        Ok(active) => active,
        Err(error) => return problem(error),
    };
    request.extensions_mut().insert(active.context.clone());
    let context = active.context.clone();
    let timeout = Duration::from_millis(stack.config().settings.timeout_ms);
    let future = run_with_context(context, async move {
        AssertUnwindSafe(next.run(request)).catch_unwind().await
    });
    let mut response = match tokio::time::timeout(timeout, future).await {
        Err(_) => problem(MiddlewareError {
            status: 504,
            code: "deadline_exceeded",
            message: "request deadline exceeded".into(),
        }),
        Ok(Err(_)) => problem(MiddlewareError {
            status: 500,
            code: "internal_error",
            message: "request handler failed".into(),
        }),
        Ok(Ok(response)) => response,
    };
    let headers = stack
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
    let status = StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(json!({
            "type": format!("urn:ores:middleware:{}", error.code),
            "title": error.code,
            "status": status.as_u16(),
            "detail": error.message
        })),
    )
        .into_response()
}

pub type AxumBody = Body;

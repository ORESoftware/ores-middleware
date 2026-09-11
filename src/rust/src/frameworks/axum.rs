use std::{
    collections::BTreeMap,
    net::SocketAddr,
    panic::AssertUnwindSafe,
    sync::Arc,
    time::Duration,
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
use serde_json::json;
#[cfg(feature = "compression")]
use tower_http::compression::CompressionLayer;

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
    let max_body_bytes = stack.config().settings.max_body_bytes;
    #[cfg(feature = "compression")]
    let compression_enabled = stack.config().settings.compression.enabled;

    let router = router
        .layer(middleware::from_fn_with_state(stack, dispatch))
        .layer(DefaultBodyLimit::max(max_body_bytes));

    #[cfg(feature = "compression")]
    if compression_enabled {
        return router.layer(CompressionLayer::new());
    }

    router
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
            // Route handlers can impose representation-specific CSP constraints
            // such as `sandbox`. Preserve both policies rather than replacing the
            // handler policy with the middleware baseline: browsers enforce
            // multiple CSP fields cumulatively, so this cannot weaken either one.
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
            [(
                "content-security-policy".to_owned(),
                "default-src 'self'; frame-ancestors 'none'".to_owned(),
            )],
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
    fn non_csp_finish_headers_keep_existing_overwrite_semantics() {
        let mut response = Response::new(Body::empty());
        response
            .headers_mut()
            .insert("x-frame-options", HeaderValue::from_static("SAMEORIGIN"));

        apply_finish_headers(
            &mut response,
            [("x-frame-options".to_owned(), "DENY".to_owned())],
        );

        assert_eq!(
            response
                .headers()
                .get("x-frame-options")
                .and_then(|value| value.to_str().ok()),
            Some("DENY")
        );
    }
}

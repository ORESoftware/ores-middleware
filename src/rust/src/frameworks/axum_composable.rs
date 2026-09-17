use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};

use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::{RequestMetadata, StaticAuthVerifier, TransportSecurity};

/// State for the standalone Axum authentication primitive.
///
/// The concrete provider type is retained inside `Arc<P>` rather than erased to
/// `Arc<dyn AuthVerifier>`. `Arc` is only for cheap state cloning. Consumers may
/// still choose a trait object as `P` when runtime heterogeneity is useful.
pub struct AuthLayerState<P> {
    verifier: Arc<P>,
}

impl<P> Clone for AuthLayerState<P> {
    fn clone(&self) -> Self {
        Self {
            verifier: Arc::clone(&self.verifier),
        }
    }
}

impl<P> AuthLayerState<P>
where
    P: StaticAuthVerifier,
{
    #[must_use]
    pub fn from_provider(provider: P) -> Self {
        Self {
            verifier: Arc::new(provider),
        }
    }

    #[must_use]
    pub fn from_shared(provider: Arc<P>) -> Self {
        Self { verifier: provider }
    }

    #[must_use]
    pub fn verifier(&self) -> &P {
        self.verifier.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestMetadataError {
    code: &'static str,
}

impl RequestMetadataError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        self.code
    }
}

/// Standalone Axum authentication middleware.
/// Consumers choose where this layer appears in their own Tower/Axum chain.
/// Dispatch is static when `P` is concrete and dynamic when the consumer passes
/// a trait-object adapter such as `Arc<dyn AuthVerifier>`.
pub async fn authenticate<P>(
    State(state): State<AuthLayerState<P>>,
    mut request: Request,
    next: Next,
) -> Response
where
    P: StaticAuthVerifier + 'static,
{
    let metadata = match request_metadata(&request) {
        Ok(metadata) => metadata,
        Err(error) => {
            tracing::warn!(
                code = error.code(),
                method = %request.method(),
                path = %request.uri().path(),
                "request rejected before authentication metadata projection"
            );
            return request_admission_problem(error.code());
        }
    };

    match state.verifier.verify_owned(metadata.clone()).await {
        Ok(decision) => {
            request.extensions_mut().insert(decision);
            next.run(request).await
        }
        Err(error) => {
            tracing::warn!(
                code = error.code,
                method = %metadata.method,
                path = %metadata.path,
                "authentication provider rejected request"
            );
            authentication_problem()
        }
    }
}

/// Build the canonical request metadata only after rejecting raw-header
/// ambiguity. Security-sensitive duplicates must never be collapsed into the
/// simple metadata map because frameworks and proxies can normalize duplicate
/// values differently.
pub fn request_metadata(request: &Request) -> Result<RequestMetadata, RequestMetadataError> {
    validate_raw_headers(request.headers())?;

    let mut headers = BTreeMap::new();
    for (name, value) in request.headers() {
        let value = value.to_str().map_err(|_| RequestMetadataError {
            code: "ores.request.invalid_header_value",
        })?;
        headers.insert(name.as_str().to_ascii_lowercase(), value.to_owned());
    }

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
    Ok(RequestMetadata {
        method: request.method().to_string(),
        path: request.uri().path().to_owned(),
        headers,
        remote_ip,
        content_length,
        transport_secure,
    })
}

fn validate_raw_headers(headers: &HeaderMap) -> Result<(), RequestMetadataError> {
    let has_content_length = headers.contains_key("content-length");
    let has_transfer_encoding = headers.contains_key("transfer-encoding");
    if has_content_length && has_transfer_encoding {
        return Err(RequestMetadataError {
            code: "ores.request.transfer_encoding_content_length_ambiguous",
        });
    }

    let mut counts = BTreeMap::<String, usize>::new();
    for (name, value) in headers {
        let name = name.as_str().to_ascii_lowercase();
        if is_security_sensitive_header(&name) && value.to_str().is_err() {
            return Err(RequestMetadataError {
                code: "ores.request.invalid_sensitive_header_value",
            });
        }
        *counts.entry(name).or_default() += 1;
    }

    for (name, count) in counts {
        if count > 1 && is_security_sensitive_header(&name) {
            return Err(RequestMetadataError {
                code: "ores.request.ambiguous_duplicate_header",
            });
        }
    }

    Ok(())
}

fn is_security_sensitive_header(name: &str) -> bool {
    matches!(
        name,
        "authorization"
            | "cookie"
            | "host"
            | "origin"
            | "forwarded"
            | "content-length"
            | "transfer-encoding"
            | "cf-connecting-ip"
            | "traceparent"
            | "tracestate"
            | "idempotency-key"
    ) || name.starts_with("x-forwarded-")
        || name.starts_with("x-ores-")
}

fn authentication_problem() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "type": "urn:ores:middleware:authentication_failed",
            "title": "authentication_failed",
            "status": StatusCode::UNAUTHORIZED.as_u16(),
            "detail": "authentication failed"
        })),
    )
        .into_response()
}

fn request_admission_problem(code: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "type": format!("urn:ores:middleware:{code}"),
            "title": "invalid_request",
            "status": StatusCode::BAD_REQUEST.as_u16(),
            "detail": "request rejected by middleware admission policy",
            "code": code
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use axum::{Router, extract::Extension, middleware, routing::get};
    use tower::{ServiceBuilder, ServiceExt};

    use super::*;
    use crate::{AuthDecision, IntegrationError, auth_provider_fn, dyn_auth_provider};

    async fn identity(Extension(identity): Extension<AuthDecision>) -> String {
        format!(
            "{}:{}",
            identity.user_id.as_deref().unwrap_or("anonymous"),
            identity.tenant_id.as_deref().unwrap_or("none")
        )
    }

    #[tokio::test]
    async fn standalone_auth_primitive_exposes_stable_decision_downstream() {
        let provider = auth_provider_fn(|request: RequestMetadata| async move {
            assert_eq!(request.path, "/me");
            Ok(AuthDecision {
                user_id: Some("alice".into()),
                tenant_id: Some("tenant-a".into()),
                claims: BTreeMap::new(),
            })
        });
        let state = AuthLayerState::from_provider(provider);
        let app = Router::new().route("/me", get(identity)).layer(
            middleware::from_fn_with_state(state, authenticate),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn provider_failure_is_fail_closed_without_exposing_provider_message() {
        let provider = auth_provider_fn(|_request: RequestMetadata| async move {
            Err(IntegrationError {
                code: "sdk_token_rejected",
                message: "secret provider diagnostic: tenant-internal-key-id=42".into(),
            })
        });
        let state = AuthLayerState::from_provider(provider);
        let app = Router::new()
            .route("/me", get(|| async { "unreachable" }))
            .layer(middleware::from_fn_with_state(state, authenticate));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("authentication_failed"));
        assert!(!body.contains("sdk_token_rejected"));
        assert!(!body.contains("tenant-internal-key-id"));
    }

    #[tokio::test]
    async fn duplicate_sensitive_headers_are_rejected_before_auth_provider() {
        let invocations = Arc::new(AtomicUsize::new(0));
        let provider_invocations = Arc::clone(&invocations);
        let provider = auth_provider_fn(move |_request: RequestMetadata| {
            let provider_invocations = Arc::clone(&provider_invocations);
            async move {
                provider_invocations.fetch_add(1, Ordering::SeqCst);
                Ok(AuthDecision::default())
            }
        });
        let state = AuthLayerState::from_provider(provider);
        let app = Router::new()
            .route("/me", get(|| async { "unreachable" }))
            .layer(middleware::from_fn_with_state(state, authenticate));
        let mut request = Request::builder()
            .uri("/me")
            .body(axum::body::Body::empty())
            .unwrap();
        request
            .headers_mut()
            .append("authorization", "Bearer first".parse().unwrap());
        request
            .headers_mut()
            .append("authorization", "Bearer second".parse().unwrap());

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(invocations.load(Ordering::SeqCst), 0);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("ores.request.ambiguous_duplicate_header"));
        assert!(!body.contains("Bearer first"));
        assert!(!body.contains("Bearer second"));
    }

    #[tokio::test]
    async fn x_ores_and_forwarding_header_duplicates_fail_closed() {
        for header in [
            "x-ores-request-id",
            "x-forwarded-for",
            "forwarded",
            "cf-connecting-ip",
            "traceparent",
            "idempotency-key",
        ] {
            let mut request = Request::builder()
                .uri("/me")
                .body(axum::body::Body::empty())
                .unwrap();
            request.headers_mut().append(header, "a".parse().unwrap());
            request.headers_mut().append(header, "b".parse().unwrap());
            let error = request_metadata(&request).unwrap_err();
            assert_eq!(error.code(), "ores.request.ambiguous_duplicate_header");
        }
    }

    #[tokio::test]
    async fn transfer_encoding_with_content_length_fails_before_projection() {
        let mut request = Request::builder()
            .uri("/me")
            .body(axum::body::Body::empty())
            .unwrap();
        request
            .headers_mut()
            .append("content-length", "4".parse().unwrap());
        request
            .headers_mut()
            .append("transfer-encoding", "chunked".parse().unwrap());
        let error = request_metadata(&request).unwrap_err();
        assert_eq!(
            error.code(),
            "ores.request.transfer_encoding_content_length_ambiguous"
        );
    }

    #[tokio::test]
    async fn concrete_provider_composes_inside_consumer_owned_service_builder() {
        let provider = auth_provider_fn(|_request: RequestMetadata| async move {
            Ok(AuthDecision {
                user_id: Some("ordered-user".into()),
                tenant_id: Some("ordered-tenant".into()),
                claims: BTreeMap::new(),
            })
        });
        let state = AuthLayerState::from_provider(provider);
        let consumer_owned_stack =
            ServiceBuilder::new().layer(middleware::from_fn_with_state(state, authenticate));
        let app = Router::new()
            .route("/me", get(identity))
            .layer(consumer_owned_stack);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn dyn_provider_uses_the_same_axum_composition_surface() {
        let provider = auth_provider_fn(|_request: RequestMetadata| async move {
            Ok(AuthDecision {
                user_id: Some("dyn-user".into()),
                tenant_id: Some("dyn-tenant".into()),
                claims: BTreeMap::new(),
            })
        });
        let provider = dyn_auth_provider(provider);
        let state = AuthLayerState::from_provider(provider);
        let app = Router::new().route("/me", get(identity)).layer(
            middleware::from_fn_with_state(state, authenticate),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}

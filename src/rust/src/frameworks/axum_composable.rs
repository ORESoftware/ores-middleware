use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};

use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::{RequestMetadata, StaticAuthVerifier, TransportSecurity};

/// State for the standalone Axum authentication primitive.
///
/// The concrete provider type is retained inside `Arc<P>` rather than erased to
/// `Arc<dyn AuthVerifier>`. `Arc` is used for cheap state cloning; dispatch is
/// still static and the provider future remains its concrete associated type.
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

/// Standalone Axum authentication middleware with static provider dispatch.
///
/// Compose this directly with `middleware::from_fn_with_state` at the exact
/// route/router boundary and ordering selected by the consuming service. Keeping
/// Axum's concrete `FromFnLayer` in the consumer expression preserves all of its
/// `Service<Request>` bounds; wrapping it behind an opaque `impl Layer` would
/// erase associated-service information that `Router::layer` needs for type
/// checking.
///
/// The layer can also be placed inside a consumer-owned `tower::ServiceBuilder`.
/// No boxed service or ORES-owned `dyn Service` boundary is required.
pub async fn authenticate<P>(
    State(state): State<AuthLayerState<P>>,
    mut request: Request,
    next: Next,
) -> Response
where
    P: StaticAuthVerifier + 'static,
{
    let metadata = request_metadata(&request);
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

/// Extract the stable ORES request metadata used by provider adapters.
///
/// Header names are canonicalized to lowercase while transport security is
/// derived only from the request URI or trusted listener-inserted extension.
#[must_use]
pub fn request_metadata(request: &Request) -> RequestMetadata {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use axum::{Router, extract::Extension, middleware, routing::get};
    use tower::{ServiceBuilder, ServiceExt};

    use super::*;
    use crate::{AuthDecision, IntegrationError, auth_provider_fn};

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
            assert_eq!(request.method, "GET");
            assert_eq!(request.path, "/me");
            assert_eq!(
                request.headers.get("authorization").map(String::as_str),
                Some("version-pinned-token")
            );
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
                    .header("authorization", "version-pinned-token")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(body.as_ref(), b"alice:tenant-a");
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
        let app = Router::new().route("/me", get(|| async { "unreachable" })).layer(
            middleware::from_fn_with_state(state, authenticate),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body = String::from_utf8(body.to_vec()).expect("utf-8 problem body");
        assert!(body.contains("authentication_failed"));
        assert!(!body.contains("sdk_token_rejected"));
        assert!(!body.contains("tenant-internal-key-id"));
    }

    #[tokio::test]
    async fn concrete_axum_layer_composes_inside_consumer_owned_service_builder() {
        let provider = auth_provider_fn(|_request: RequestMetadata| async move {
            Ok(AuthDecision {
                user_id: Some("ordered-user".into()),
                tenant_id: Some("ordered-tenant".into()),
                claims: BTreeMap::new(),
            })
        });

        let state = AuthLayerState::from_provider(provider);
        let consumer_owned_stack = ServiceBuilder::new().layer(
            middleware::from_fn_with_state(state, authenticate),
        );
        let app = Router::new()
            .route("/me", get(identity))
            .layer(consumer_owned_stack);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(body.as_ref(), b"ordered-user:ordered-tenant");
    }
}

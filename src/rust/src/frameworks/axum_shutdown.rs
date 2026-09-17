use axum::{
    body::Body,
    extract::{Request, State},
    http::{
        HeaderName, HeaderValue, StatusCode, Version,
        header::CONNECTION,
    },
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{ShutdownCoordinator, ShutdownRejection, hardening::sanitized_problem_response};

#[derive(Clone, Debug)]
pub struct ShutdownLayerState {
    coordinator: ShutdownCoordinator,
}

impl ShutdownLayerState {
    #[must_use]
    pub fn new(coordinator: ShutdownCoordinator) -> Self {
        Self { coordinator }
    }

    #[must_use]
    pub fn coordinator(&self) -> &ShutdownCoordinator {
        &self.coordinator
    }
}

/// Reject requests that arrive after graceful draining begins while retaining a
/// guard for requests already admitted. Consumers still own listener shutdown,
/// signal handling, layer ordering, and the final process/telemetry flush.
pub async fn admit_during_shutdown(
    State(state): State<ShutdownLayerState>,
    request: Request,
    next: Next,
) -> Response {
    let version = request.version();
    let _guard = match state.coordinator.begin_request() {
        Ok(guard) => guard,
        Err(rejection) => return shutdown_response(rejection, version),
    };

    next.run(request).await
}

fn shutdown_response(rejection: ShutdownRejection, version: Version) -> Response {
    let stage_response = sanitized_problem_response(
        rejection.status,
        rejection.code,
        None,
        &rejection.headers,
    );
    let status = StatusCode::from_u16(stage_response.status)
        .unwrap_or(StatusCode::TOO_MANY_REQUESTS);
    let mut response = (status, Body::from(stage_response.body)).into_response();

    for (name, value) in stage_response.headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            response.headers_mut().insert(name, value);
        }
    }

    // `Connection` is hop-by-hop metadata, so it is owned by this transport
    // adapter rather than the generic shutdown rejection. HTTP/2 and HTTP/3
    // prohibit the field entirely.
    if matches!(version, Version::HTTP_10 | Version::HTTP_11) {
        response
            .headers_mut()
            .insert(CONNECTION, HeaderValue::from_static("close"));
    }

    response
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, Version, header::{CACHE_CONTROL, CONNECTION, CONTENT_TYPE, RETRY_AFTER}},
        middleware,
        routing::get,
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;

    fn app(coordinator: &ShutdownCoordinator) -> Router {
        let state = ShutdownLayerState::new(coordinator.clone());
        Router::new()
            .route("/", get(|| async { "ok" }))
            .route_layer(middleware::from_fn_with_state(state, admit_during_shutdown))
    }

    #[tokio::test]
    async fn axum_layer_tracks_existing_requests_and_rejects_after_drain() {
        let coordinator = ShutdownCoordinator::default();
        let app = app(&coordinator);

        let running = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("running response");
        assert_eq!(running.status(), StatusCode::OK);
        assert_eq!(coordinator.active_requests(), 0);

        coordinator.start_draining();
        let draining = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .version(Version::HTTP_11)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("draining response");
        assert_eq!(draining.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            draining.headers().get(CONNECTION).and_then(|value| value.to_str().ok()),
            Some("close")
        );
        assert_eq!(
            draining.headers().get(RETRY_AFTER).and_then(|value| value.to_str().ok()),
            Some("5")
        );
        assert_eq!(
            draining.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            Some("application/problem+json")
        );
        assert_eq!(
            draining.headers().get(CACHE_CONTROL).and_then(|value| value.to_str().ok()),
            Some("no-store")
        );

        let body = to_bytes(draining.into_body(), 4096)
            .await
            .expect("bounded problem body");
        let problem: Value = serde_json::from_slice(&body).expect("valid problem json");
        assert_eq!(problem["status"], 429);
        assert_eq!(problem["code"], "service_draining");
        assert_eq!(problem["title"], "Request rejected");
    }

    #[tokio::test]
    async fn http2_shutdown_rejection_omits_connection_specific_header() {
        let coordinator = ShutdownCoordinator::default();
        coordinator.start_draining();

        let response = app(&coordinator)
            .oneshot(
                Request::builder()
                    .uri("/")
                    .version(Version::HTTP_2)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("draining response");

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(!response.headers().contains_key(CONNECTION));
        assert_eq!(
            response.headers().get(RETRY_AFTER).and_then(|value| value.to_str().ok()),
            Some("5")
        );
    }
}

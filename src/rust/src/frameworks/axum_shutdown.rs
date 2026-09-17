use axum::{
    Json,
    extract::{Request, State},
    http::{
        HeaderValue, StatusCode,
        header::{CONNECTION, RETRY_AFTER},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::{ShutdownCoordinator, ShutdownRejection};

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
    let _guard = match state.coordinator.begin_request() {
        Ok(guard) => guard,
        Err(rejection) => return shutdown_response(rejection),
    };

    next.run(request).await
}

fn shutdown_response(rejection: ShutdownRejection) -> Response {
    let mut response = (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": {
                "code": rejection.code,
                "message": rejection.message,
            }
        })),
    )
        .into_response();

    response
        .headers_mut()
        .insert(CONNECTION, HeaderValue::from_static("close"));
    if let Some(value) = rejection.headers.get("retry-after")
        && let Ok(value) = HeaderValue::from_str(value)
    {
        response.headers_mut().insert(RETRY_AFTER, value);
    }
    response.headers_mut().insert(
        "x-ores-error-code",
        HeaderValue::from_static("service_draining"),
    );
    response
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::Request,
        middleware,
        routing::get,
    };
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn axum_layer_tracks_existing_requests_and_rejects_after_drain() {
        let coordinator = ShutdownCoordinator::default();
        let state = ShutdownLayerState::new(coordinator.clone());
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .route_layer(middleware::from_fn_with_state(state, admit_during_shutdown));

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
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("draining response");
        assert_eq!(draining.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(draining.headers().get(CONNECTION).and_then(|v| v.to_str().ok()), Some("close"));
        assert_eq!(draining.headers().get(RETRY_AFTER).and_then(|v| v.to_str().ok()), Some("5"));
        assert_eq!(
            draining
                .headers()
                .get("x-ores-error-code")
                .and_then(|v| v.to_str().ok()),
            Some("service_draining")
        );
    }
}

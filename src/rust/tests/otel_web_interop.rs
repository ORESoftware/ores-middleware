//! External bridge + real middleware integration. The logger is imported from
//! middleware and accepted by the bridge, which also proves Cargo type identity.
#![cfg(all(feature = "axum", not(target_arch = "wasm32")))]

use axum::{
    Json, Router,
    body::Body,
    extract::Extension,
    http::{Request, StatusCode},
    routing::get,
};
use ores_middleware::{RuntimeEnvironment, default_config, frameworks::axum_audit, otel};
use ores_otel_web::{TraceParent, server::install_with_logger};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

#[derive(Default)]
struct Capture(Mutex<Vec<otel::LogRecord>>);
impl otel::Transport for Capture {
    fn write(&self, record: &otel::LogRecord) -> Result<(), otel::LoggerError> {
        self.0.lock().unwrap().push(record.clone());
        Ok(())
    }
}

#[tokio::test]
async fn middleware_and_browser_bridge_keep_interleaved_requests_correlated() {
    let capture = Arc::new(Capture::default());
    let logger = otel::Logger::new(
        otel::Options {
            app_name: "middleware-web-interop".into(),
            console: false,
            ..otel::Options::default()
        }
        .with_transport(capture.clone()),
    );
    let handler_logger = logger.clone();
    let router = Router::new().route(
        "/",
        get(move |Extension(trace): Extension<TraceParent>| {
            let logger = handler_logger.clone();
            async move {
                tokio::task::yield_now().await;
                let context = otel::current_log_context();
                assert_eq!(context.trace_id.as_deref(), Some(trace.trace_id()));
                assert!(context.logged_in_user.is_empty());
                logger.info_context(vec![otel::json!("handler.complete")]).send().unwrap();
                (StatusCode::ACCEPTED, Json(otel::json!({"ok": true})))
            }
        }),
    );
    // In-process synthetic HTTP only. Production configuration is untouched;
    // no test-auth bypass or fault injection is enabled.
    let mut config = default_config("middleware-web-interop");
    config.environment = RuntimeEnvironment::Test;
    config.settings.tls.mode = "disabled".into();
    config.settings.tls.require_https = false;
    let app = install_with_logger(axum_audit::install_with_config(router, config).unwrap(), logger);
    let parents = [
        "00-11111111111111111111111111111111-1111111111111111-00",
        "00-22222222222222222222222222222222-2222222222222222-00",
    ];
    let request = |parent| {
        Request::builder()
            .uri("/?secret=synthetic-do-not-log")
            .header("accept", "application/json")
            .header("traceparent", parent)
            .body(Body::empty())
            .unwrap()
    };
    let (a, b) = tokio::join!(
        app.clone().oneshot(request(parents[0])),
        app.oneshot(request(parents[1])),
    );
    for (response, parent) in [(a.unwrap(), parents[0]), (b.unwrap(), parents[1])] {
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let actual: TraceParent = response.headers()["traceparent"].to_str().unwrap().parse().unwrap();
        assert_eq!(actual.trace_id(), parent.parse::<TraceParent>().unwrap().trace_id());
    }
    assert!(otel::current_log_context().trace_id.is_none());
    let records = capture.0.lock().unwrap();
    assert_eq!(records.len(), 4, "one handler and one bridge record per request");
    for parent in parents {
        let expected: TraceParent = parent.parse().unwrap();
        assert_eq!(records.iter().filter(|record| record.trace_id.as_deref() == Some(expected.trace_id())).count(), 2);
    }
    for record in records.iter() {
        assert!(!record.to_json().unwrap().contains("synthetic-do-not-log"));
    }
}

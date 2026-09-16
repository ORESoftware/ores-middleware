use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Router,
    body::Body,
    extract::Extension,
    http::{Request, StatusCode},
    routing::get,
};
use ores_middleware::otel::{
    LoggerError, MemoryTransport, OpenTelemetryLogRecord, OpenTelemetryTransport, OresOtelEnv,
    ResolveOptions, RuntimeRole, Transport, Value, parse_ores_otel_toml, resolve_ores_otel_config,
    server_otel_runtime_from_resolved,
};
use ores_middleware::{
    MiddlewareStack, RequestLogger, default_config, frameworks::axum::install_with_ores_runtime,
};
use tower::ServiceExt;

fn stack() -> MiddlewareStack {
    let mut config = default_config("runtime-config-test");
    config.settings.tls.mode = "disabled".into();
    config.settings.tls.require_https = false;
    config.settings.rate_limit.enabled = false;
    config.settings.compression.enabled = false;
    MiddlewareStack::new(config).expect("valid test middleware")
}

fn resolved(input: &str) -> ores_middleware::otel::ResolvedOresOtelConfig {
    let parsed = parse_ores_otel_toml(input).expect("valid telemetry fixture");
    resolve_ores_otel_config(
        &parsed,
        &ResolveOptions::default().with_role(RuntimeRole::Server),
    )
    .expect("resolved server telemetry")
}

async fn emits_request_log(Extension(logger): Extension<RequestLogger>) -> StatusCode {
    assert!(!logger.otel_sampled());
    logger
        .info(vec![Value::String("handler log".into())])
        .send()
        .expect("non-OTel request log must remain deliverable");
    StatusCode::NO_CONTENT
}

#[tokio::test]
async fn resolved_zero_sampling_suppresses_only_otel_transport() {
    let memory = Arc::new(MemoryTransport::default());
    let otel_writes = Arc::new(AtomicUsize::new(0));
    let otel_counter = otel_writes.clone();
    let otel = Arc::new(OpenTelemetryTransport::new(
        move |_record: OpenTelemetryLogRecord| -> Result<(), LoggerError> {
            otel_counter.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
    ));
    let memory_transport: Arc<dyn Transport> = memory.clone();
    let otel_transport: Arc<dyn Transport> = otel;

    let config = resolved(
        r#"
version = 1
[server]
service_name = "runtime-config-test"
[server.logging]
enabled = true
level = "info"
console = false
[server.tracing]
enabled = true
sample_ratio = 0.0
propagators = ["tracecontext", "baggage"]
[server.exporter]
protocol = "none"
"#,
    );
    let runtime = server_otel_runtime_from_resolved(
        config,
        &OresOtelEnv::new(),
        "runtime-config-test",
        Some("http"),
        vec![memory_transport, otel_transport],
    )
    .expect("resolved runtime");

    let app = install_with_ores_runtime(
        Router::new().route("/", get(emits_request_log)),
        Arc::new(stack()),
        &runtime,
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header("accept", "application/json")
                .header("x-request-id", "runtime-config-request")
                .header(
                    "traceparent",
                    "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01",
                )
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        memory.records().len() >= 3,
        "start, handler and completion logs expected"
    );
    assert_eq!(otel_writes.load(Ordering::Relaxed), 0);
    runtime.close().expect("runtime shutdown");
    assert!(memory.is_closed());
}

#[test]
fn active_otlp_requires_credential_free_endpoint_and_explicit_transport() {
    let config = resolved(
        r#"
version = 1
[server]
service_name = "runtime-config-test"
[server.exporter]
protocol = "otlp_http"
endpoint_env = "OTEL_EXPORTER_OTLP_ENDPOINT"
"#,
    );
    let transport: Arc<dyn Transport> = Arc::new(OpenTelemetryTransport::new(
        |_record: OpenTelemetryLogRecord| -> Result<(), LoggerError> { Ok(()) },
    ));

    let valid = OresOtelEnv::from([(
        "OTEL_EXPORTER_OTLP_ENDPOINT".into(),
        "https://collector.example.com/v1/logs".into(),
    )]);
    let runtime = server_otel_runtime_from_resolved(
        config.clone(),
        &valid,
        "runtime-config-test",
        Some("http"),
        vec![transport],
    )
    .expect("valid explicit transport");
    assert_eq!(
        runtime.exporter_endpoint.as_deref(),
        Some("https://collector.example.com/v1/logs")
    );

    for invalid in [
        "https://user:password@collector.example.com/v1/logs",
        "https://collector.example.com/v1/logs?token=secret",
        "file:///tmp/otel",
    ] {
        let environment =
            OresOtelEnv::from([("OTEL_EXPORTER_OTLP_ENDPOINT".into(), invalid.to_owned())]);
        let transport: Arc<dyn Transport> = Arc::new(OpenTelemetryTransport::new(
            |_record: OpenTelemetryLogRecord| -> Result<(), LoggerError> { Ok(()) },
        ));
        assert!(
            server_otel_runtime_from_resolved(
                config.clone(),
                &environment,
                "runtime-config-test",
                None,
                vec![transport],
            )
            .is_err()
        );
    }
}

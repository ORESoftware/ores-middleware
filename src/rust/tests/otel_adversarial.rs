use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{
    body::Body,
    extract::{Extension, Request},
    http::{header::ACCEPT, HeaderValue, StatusCode},
    routing::get,
    Router,
};
use ores_middleware::{
    default_config,
    frameworks::axum::install_with_ores_logger,
    AuthDecision,
    AuthVerifier,
    IntegrationError,
    MiddlewareStack,
    RequestContext,
    RequestLogger,
    RequestMetadata,
};
use ores_middleware::otel::{
    current_log_context, JsonObject, Logger, LoggerError, MemoryTransport, Options, Transport,
    Value,
};
use tower::ServiceExt;

struct HeaderAuth;

impl AuthVerifier for HeaderAuth {
    fn verify<'a>(
        &'a self,
        request: &'a RequestMetadata,
    ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>> {
        Box::pin(async move {
            let slot = request.headers.get("x-test-slot").cloned().unwrap_or_default();
            Ok(AuthDecision {
                user_id: Some(format!("user-{slot}")),
                tenant_id: Some(format!("tenant-{slot}")),
                claims: BTreeMap::from([
                    ("otel.slot".into(), slot),
                    ("authorization".into(), "must-not-propagate".into()),
                ]),
            })
        })
    }
}

#[derive(Default)]
struct FailingTransport {
    writes: AtomicUsize,
}

impl Transport for FailingTransport {
    fn write(&self, _record: &ores_middleware::otel::LogRecord) -> Result<(), LoggerError> {
        self.writes.fetch_add(1, Ordering::Relaxed);
        Err(LoggerError("sink unavailable".into()))
    }
}

fn test_stack(timeout_ms: u64) -> MiddlewareStack {
    let mut config = default_config("middleware-adversarial-test");
    config.settings.timeout_ms = timeout_ms;
    config.settings.tls.mode = "disabled".into();
    config.settings.tls.require_https = false;
    config.settings.rate_limit.enabled = false;
    config.settings.compression.enabled = false;
    MiddlewareStack::new(config).expect("valid test config")
}

fn logger_with_transport<T: Transport + 'static>(name: &str, transport: Arc<T>) -> Logger {
    Logger::new(Options {
        app_name: "middleware-adversarial-test".into(),
        name: Some(name.into()),
        transports: vec![transport],
        console: false,
        ..Options::default()
    })
}

fn request(slot: usize, path: &str) -> Request {
    let mut request = Request::builder()
        .method("GET")
        .uri(path)
        .header(ACCEPT, "application/json")
        .header("x-request-id", format!("request-{slot}"))
        .header("x-test-slot", slot.to_string())
        .header(
            "traceparent",
            format!("00-{:032x}-0123456789abcdef-01", slot + 1),
        )
        .body(Body::empty())
        .expect("request");
    request
        .headers_mut()
        .insert("x-test-marker", HeaderValue::from_static("adversarial"));
    request
}

async fn correlated_handler(
    Extension(context): Extension<RequestContext>,
    Extension(request_logger): Extension<RequestLogger>,
    Extension(file_logger): Extension<Logger>,
    request: Request,
) -> StatusCode {
    let slot = request
        .headers()
        .get("x-test-slot")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if context.request_id != format!("request-{slot}")
        || context.user_id.as_deref() != Some(format!("user-{slot}").as_str())
        || context.tenant_id.as_deref() != Some(format!("tenant-{slot}").as_str())
    {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    let active = current_log_context();
    if active.fields.get("request.id")
        != Some(&Value::String(format!("request-{slot}")))
        || active
            .logged_in_user
            .get("id")
            != Some(&Value::String(format!("user-{slot}")))
        || active.baggage.get("otel.slot") != Some(&slot.to_string())
        || active.baggage.contains_key("authorization")
    {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    tokio::time::sleep(Duration::from_millis((slot.len() % 5) as u64)).await;
    if file_logger
        .info_context(vec![Value::String(format!("file:{slot}"))])
        .send()
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    if request_logger
        .warn(vec![Value::String(format!("request:{slot}"))])
        .send()
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::NO_CONTENT
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_axum_requests_never_cross_contaminate_log_context() {
    let transport = Arc::new(MemoryTransport::default());
    let root = logger_with_transport("server", transport.clone());
    let file_logger = logger_with_transport("handler", transport.clone());
    let stack = test_stack(2_000).with_auth_verifier(Arc::new(HeaderAuth));
    let app = install_with_ores_logger(
        Router::new()
            .route("/orders/{slot}", get(correlated_handler))
            .layer(Extension(file_logger)),
        Arc::new(stack),
        root,
    );

    let mut tasks = Vec::new();
    for slot in 0..48 {
        let app = app.clone();
        tasks.push(tokio::spawn(async move {
            app.oneshot(request(slot, &format!("/orders/{slot}")))
                .await
                .expect("response")
                .status()
        }));
    }
    for task in tasks {
        assert_eq!(task.await.expect("request task"), StatusCode::NO_CONTENT);
    }

    assert_eq!(current_log_context(), Default::default());
    let records = transport.records();
    for slot in 0..48 {
        for message in [format!("file:{slot}"), format!("request:{slot}")] {
            let matches = records
                .iter()
                .filter(|record| record.message == message)
                .collect::<Vec<_>>();
            assert_eq!(matches.len(), 1, "expected exactly one {message} record");
            let record = matches[0];
            assert_eq!(
                record.fields.get("request.id"),
                Some(&Value::String(format!("request-{slot}")))
            );
            assert_eq!(
                record.fields.get("user.id"),
                Some(&Value::String(format!("user-{slot}")))
            );
            assert_eq!(
                record.fields.get("tenant.id"),
                Some(&Value::String(format!("tenant-{slot}")))
            );
            assert_eq!(
                record
                    .logged_in_user
                    .as_ref()
                    .and_then(|user| user.get("id")),
                Some(&Value::String(format!("user-{slot}")))
            );
            let baggage = record
                .fields
                .get("otel.baggage")
                .and_then(Value::as_object)
                .expect("otel baggage");
            assert_eq!(
                baggage.get("otel.slot"),
                Some(&Value::String(slot.to_string()))
            );
            assert!(!baggage.contains_key("authorization"));
            assert!(!record
                .to_json()
                .expect("record json")
                .contains("must-not-propagate"));
        }
    }
}

async fn no_content_handler() -> StatusCode {
    StatusCode::NO_CONTENT
}

#[tokio::test]
async fn transport_failure_is_fail_open_at_the_axum_boundary() {
    let transport = Arc::new(FailingTransport::default());
    let logger = logger_with_transport("server", transport.clone());
    let app = install_with_ores_logger(
        Router::new().route("/", get(no_content_handler)),
        Arc::new(test_stack(1_000)),
        logger,
    );

    let response = app
        .oneshot(request(1, "/"))
        .await
        .expect("response despite logger failure");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(transport.writes.load(Ordering::Relaxed) >= 2);
}

async fn slow_handler() -> StatusCode {
    tokio::time::sleep(Duration::from_millis(60)).await;
    StatusCode::NO_CONTENT
}

#[tokio::test]
async fn timeout_emits_timeout_and_never_completion() {
    let transport = Arc::new(MemoryTransport::default());
    let logger = logger_with_transport("server", transport.clone());
    let app = install_with_ores_logger(
        Router::new().route("/slow", get(slow_handler)),
        Arc::new(test_stack(15)),
        logger,
    );

    let response = app
        .oneshot(request(2, "/slow"))
        .await
        .expect("timeout response");
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    let records = transport.records();
    assert!(records
        .iter()
        .any(|record| record.message == "request handler timed out"));
    assert!(!records
        .iter()
        .any(|record| record.message == "request handler completed"));
}

async fn panic_handler() -> StatusCode {
    panic!("boom");
}

#[tokio::test]
async fn panic_emits_panic_and_never_completion() {
    let transport = Arc::new(MemoryTransport::default());
    let logger = logger_with_transport("server", transport.clone());
    let app = install_with_ores_logger(
        Router::new().route("/panic", get(panic_handler)),
        Arc::new(test_stack(1_000)),
        logger,
    );

    let response = app
        .oneshot(request(3, "/panic"))
        .await
        .expect("panic recovery response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let records = transport.records();
    assert!(records
        .iter()
        .any(|record| record.message == "request handler panicked"));
    assert!(!records
        .iter()
        .any(|record| record.message == "request handler completed"));
}

#[test]
fn log_context_mapping_does_not_mutate_the_portable_input() {
    let context = RequestContext {
        request_id: "request-immutable".into(),
        trace_id: "0123456789abcdef0123456789abcdef".into(),
        span_id: Some("0123456789abcdef".into()),
        tenant_id: Some("tenant-immutable".into()),
        user_id: Some("user-immutable".into()),
        locale: None,
        started_at_unix_ms: 1,
        deadline_unix_ms: Some(2),
        baggage: BTreeMap::from([
            ("otel.allowed".into(), "yes".into()),
            ("authorization".into(), "must-not-propagate".into()),
        ]),
    };
    let before = context.clone();
    let mapped = ores_middleware::to_ores_log_context(&context);
    assert_eq!(context.request_id, before.request_id);
    assert_eq!(context.baggage, before.baggage);
    assert_eq!(mapped.baggage.get("otel.allowed"), Some(&"yes".into()));
    assert!(!mapped.baggage.contains_key("authorization"));
    assert_eq!(mapped.fields, JsonObject::from_iter(mapped.fields.clone()));
}

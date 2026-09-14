use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use ores_middleware::{
    CircuitBreakerConfig, MiddlewareStageHandler, RequestContext, RequestMetadata, StageDecision,
    StageInput, StageResponse,
    hardening::{
        BreakerKey, CircuitBreakerRegistry, CircuitBreakerRegistryConfig, FetchMetadataPolicy,
        FetchMetadataStage, FinalizerFailureMode, FinalizerOutcome, HardenedStagePipeline,
        LifecycleFailureKind, MAX_BREAKER_REGISTRY_ENTRIES, MAX_HARDENED_STAGES,
        MAX_STAGE_ATTRIBUTE_KEY_BYTES, MAX_STAGE_ATTRIBUTE_VALUE_BYTES, admit_raw_headers,
        sanitized_problem_response, validate_stage_attributes,
    },
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

fn pair(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

fn input(method: &str, headers: &[(&str, &str)]) -> StageInput {
    StageInput::new(
        RequestMetadata {
            method: method.to_owned(),
            path: "/widgets".to_owned(),
            headers: headers
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            remote_ip: Some("127.0.0.1".to_owned()),
            content_length: None,
            transport_secure: true,
        },
        RequestContext {
            request_id: "req-1".to_owned(),
            trace_id: "0123456789abcdef0123456789abcdef".to_owned(),
            span_id: None,
            tenant_id: None,
            user_id: None,
            locale: None,
            started_at_unix_ms: 0,
            deadline_unix_ms: None,
            baggage: BTreeMap::new(),
        },
    )
}

async fn fetch_decision(policy: FetchMetadataPolicy, request: StageInput) -> StageDecision {
    FetchMetadataStage::new(policy).request(request).await
}

struct PassStage(&'static str);

impl MiddlewareStageHandler for PassStage {
    fn name(&self) -> &'static str {
        self.0
    }

    fn request<'a>(
        &'a self,
        input: StageInput,
    ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
        Box::pin(async move { StageDecision::Continue(Box::new(input)) })
    }
}

struct PanicFinalizer;

impl MiddlewareStageHandler for PanicFinalizer {
    fn name(&self) -> &'static str {
        "panic-finalizer"
    }

    fn request<'a>(
        &'a self,
        input: StageInput,
    ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
        Box::pin(async move { StageDecision::Continue(Box::new(input)) })
    }

    fn response<'a>(
        &'a self,
        _input: &'a StageInput,
        _response: StageResponse,
    ) -> Pin<Box<dyn Future<Output = StageResponse> + Send + 'a>> {
        Box::pin(async move { panic!("adversarial finalizer panic") })
    }
}

struct RecordingFinalizer {
    calls: Arc<AtomicUsize>,
    names: Arc<Mutex<Vec<&'static str>>>,
}

impl MiddlewareStageHandler for RecordingFinalizer {
    fn name(&self) -> &'static str {
        "recording-finalizer"
    }

    fn request<'a>(
        &'a self,
        input: StageInput,
    ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
        Box::pin(async move { StageDecision::Continue(Box::new(input)) })
    }

    fn response<'a>(
        &'a self,
        _input: &'a StageInput,
        mut response: StageResponse,
    ) -> Pin<Box<dyn Future<Output = StageResponse> + Send + 'a>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.names.lock().await.push(self.name());
            response
                .headers
                .insert("x-test-finalized".to_owned(), "true".to_owned());
            response
        })
    }
}

struct SlowRequestStage;

impl MiddlewareStageHandler for SlowRequestStage {
    fn name(&self) -> &'static str {
        "slow-request"
    }

    fn request<'a>(
        &'a self,
        input: StageInput,
    ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            StageDecision::Continue(Box::new(input))
        })
    }
}

struct OverflowAttributeStage;

impl MiddlewareStageHandler for OverflowAttributeStage {
    fn name(&self) -> &'static str {
        "overflow-attribute"
    }

    fn request<'a>(
        &'a self,
        input: StageInput,
    ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
        Box::pin(async move {
            StageDecision::Continue(Box::new(input.with_attribute(
                "payload",
                json!("x".repeat(MAX_STAGE_ATTRIBUTE_VALUE_BYTES + 1)),
            )))
        })
    }
}

fn registry_config(max_entries: usize, idle_ttl: Duration) -> CircuitBreakerRegistryConfig {
    CircuitBreakerRegistryConfig {
        max_entries,
        idle_ttl,
        breaker: CircuitBreakerConfig {
            failure_threshold: 2,
            open_for: Duration::from_secs(1),
        },
    }
}

#[test]
fn reserved_ores_headers_cannot_be_collapsed_case_insensitively() {
    let error = admit_raw_headers(&[
        pair("X-ORES-Request-ID", "one"),
        pair("x-ores-request-id", "two"),
    ])
    .unwrap_err();
    assert_eq!(error.code, "duplicate_header_forbidden");
}

#[test]
fn forwarded_headers_cannot_be_collapsed_case_insensitively() {
    let error = admit_raw_headers(&[
        pair("X-Forwarded-For", "192.0.2.1"),
        pair("x-forwarded-for", "198.51.100.4"),
    ])
    .unwrap_err();
    assert_eq!(error.code, "duplicate_header_forbidden");
}

#[test]
fn invalid_header_name_and_del_value_are_rejected_before_map_projection() {
    assert_eq!(
        admit_raw_headers(&[pair("bad header", "value")])
            .unwrap_err()
            .code,
        "header_name_invalid"
    );
    let del = format!("safe{}", '\u{7f}');
    assert_eq!(
        admit_raw_headers(&[pair("x-test", &del)])
            .unwrap_err()
            .code,
        "header_value_invalid"
    );
}

#[test]
fn aggregate_attribute_budget_is_enforced_independently_of_per_value_budget() {
    let attributes = (0..10)
        .map(|index| (format!("key_{index}"), json!("x".repeat(900))))
        .collect::<BTreeMap<String, Value>>();
    assert_eq!(
        validate_stage_attributes(&attributes).unwrap_err().code,
        "stage_attribute_bytes_exceeded"
    );
}

#[test]
fn exact_attribute_key_boundary_remains_valid() {
    let key = "a".repeat(MAX_STAGE_ATTRIBUTE_KEY_BYTES);
    let attributes = BTreeMap::from([(key, json!(1))]);
    validate_stage_attributes(&attributes).unwrap();
}

#[test]
fn problem_response_preserves_only_safe_bounded_metadata() {
    let safe_headers = BTreeMap::from([
        ("x-ores-request-id".to_owned(), "ores-123".to_owned()),
        ("set-cookie".to_owned(), "secret=true".to_owned()),
        ("retry-after".to_owned(), "3".to_owned()),
    ]);
    let response = sanitized_problem_response(
        429,
        "rate limit failed: bearer secret",
        Some("bad\r\nrequest-id"),
        &safe_headers,
    );
    let body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response.status, 429);
    assert_eq!(
        response.headers.get("x-ores-request-id").map(String::as_str),
        Some("ores-123")
    );
    assert_eq!(
        response.headers.get("retry-after").map(String::as_str),
        Some("3")
    );
    assert!(!response.headers.contains_key("set-cookie"));
    assert!(body.get("requestId").is_none());
    assert!(!String::from_utf8_lossy(&response.body).contains("bearer secret"));
}

#[tokio::test]
async fn fetch_metadata_can_deny_same_site_ambient_mutations() {
    let decision = fetch_decision(
        FetchMetadataPolicy {
            allow_same_site: false,
            ..FetchMetadataPolicy::default()
        },
        input(
            "POST",
            &[("cookie", "session=s1"), ("sec-fetch-site", "same-site")],
        ),
    )
    .await;
    assert!(matches!(decision, StageDecision::Reject(_)));
}

#[tokio::test]
async fn fetch_metadata_does_not_treat_bearer_auth_as_ambient_credentials() {
    let decision = fetch_decision(
        FetchMetadataPolicy::default(),
        input(
            "POST",
            &[
                ("authorization", "Bearer token"),
                ("sec-fetch-site", "cross-site"),
            ],
        ),
    )
    .await;
    assert!(matches!(decision, StageDecision::Continue(_)));
}

#[tokio::test]
async fn disabled_fetch_metadata_policy_is_a_true_bypass() {
    let decision = fetch_decision(
        FetchMetadataPolicy {
            enabled: false,
            ..FetchMetadataPolicy::default()
        },
        input(
            "POST",
            &[
                ("cookie", "session=s1"),
                ("sec-fetch-site", "MALFORMED VALUE"),
            ],
        ),
    )
    .await;
    assert!(matches!(decision, StageDecision::Continue(_)));
}

#[tokio::test]
async fn fail_open_finalizer_panic_preserves_application_response_and_records_failure() {
    let pipeline = HardenedStagePipeline::new()
        .try_with_stage(Arc::new(PanicFinalizer), FinalizerFailureMode::FailOpen)
        .unwrap();
    let execution = pipeline
        .execute(input("GET", &[]), |_| async { StageResponse::empty(204) })
        .await;
    assert_eq!(execution.response.status, 204);
    assert_eq!(execution.finalizers.len(), 1);
    assert_eq!(
        execution.finalizers[0].outcome,
        FinalizerOutcome::Failed(LifecycleFailureKind::Panic)
    );
}

#[tokio::test]
async fn fail_closed_finalizer_still_runs_earlier_cleanup_without_overwriting_terminal_failure() {
    let calls = Arc::new(AtomicUsize::new(0));
    let names = Arc::new(Mutex::new(Vec::new()));
    let pipeline = HardenedStagePipeline::new()
        .try_with_stage(
            Arc::new(RecordingFinalizer {
                calls: Arc::clone(&calls),
                names: Arc::clone(&names),
            }),
            FinalizerFailureMode::FailOpen,
        )
        .unwrap()
        .try_with_stage(Arc::new(PanicFinalizer), FinalizerFailureMode::FailClosed)
        .unwrap();

    let execution = pipeline
        .execute(input("GET", &[]), |_| async { StageResponse::empty(200) })
        .await;

    assert_eq!(execution.response.status, 500);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(*names.lock().await, vec!["recording-finalizer"]);
    assert!(!execution.response.headers.contains_key("x-test-finalized"));
    assert_eq!(execution.finalizers.len(), 2);
    assert_eq!(
        execution.finalizers[0].outcome,
        FinalizerOutcome::Failed(LifecycleFailureKind::Panic)
    );
    assert_eq!(execution.finalizers[1].outcome, FinalizerOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn already_expired_deadline_aborts_a_slow_request_stage() {
    let mut request = input("GET", &[]);
    request.context.deadline_unix_ms = Some(0);
    let pipeline = HardenedStagePipeline::new()
        .try_with_stage(Arc::new(SlowRequestStage), FinalizerFailureMode::FailClosed)
        .unwrap();
    let execution = pipeline
        .execute(request, |_| async { StageResponse::empty(200) })
        .await;
    assert_eq!(execution.response.status, 500);
    assert!(String::from_utf8_lossy(&execution.response.body).contains("stage_deadline_exceeded"));
}

#[tokio::test]
async fn attributes_added_by_a_stage_are_revalidated_before_the_handler_runs() {
    let handler_calls = Arc::new(AtomicUsize::new(0));
    let pipeline = HardenedStagePipeline::new()
        .try_with_stage(
            Arc::new(OverflowAttributeStage),
            FinalizerFailureMode::FailClosed,
        )
        .unwrap();
    let handler_counter = Arc::clone(&handler_calls);
    let execution = pipeline
        .execute(input("POST", &[]), move |_| {
            handler_counter.fetch_add(1, Ordering::SeqCst);
            async { StageResponse::empty(200) }
        })
        .await;
    assert_eq!(execution.response.status, 500);
    assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
    assert!(String::from_utf8_lossy(&execution.response.body).contains("stage_attributes_invalid"));
}

#[tokio::test]
async fn breaker_registry_reuses_existing_key_without_unbounded_growth() {
    let registry = CircuitBreakerRegistry::new(registry_config(2, Duration::from_secs(60))).unwrap();
    let key = BreakerKey::new("shared-auth", "verify").unwrap();
    registry.get_or_insert(key.clone()).await.unwrap();
    registry.get_or_insert(key).await.unwrap();
    assert_eq!(registry.len().await, 1);
    assert_eq!(registry.eviction_count(), 0);
}

#[tokio::test]
async fn breaker_registry_capacity_is_hard_bounded_and_invalid_capacities_fail_closed() {
    assert_eq!(
        CircuitBreakerRegistry::new(registry_config(0, Duration::from_secs(1)))
            .err()
            .unwrap()
            .code,
        "circuit_registry_capacity_invalid"
    );
    assert_eq!(
        CircuitBreakerRegistry::new(registry_config(
            MAX_BREAKER_REGISTRY_ENTRIES + 1,
            Duration::from_secs(1),
        ))
        .err()
        .unwrap()
        .code,
        "circuit_registry_capacity_invalid"
    );

    let registry = CircuitBreakerRegistry::new(registry_config(1, Duration::from_secs(60))).unwrap();
    registry
        .get_or_insert(BreakerKey::new("auth", "verify").unwrap())
        .await
        .unwrap();
    registry
        .get_or_insert(BreakerKey::new("cache", "read").unwrap())
        .await
        .unwrap();
    assert_eq!(registry.len().await, 1);
    assert_eq!(registry.eviction_count(), 1);
}

#[test]
fn hardened_pipeline_accepts_exact_stage_bound_and_rejects_one_more() {
    let mut pipeline = HardenedStagePipeline::new();
    for _ in 0..MAX_HARDENED_STAGES {
        pipeline = pipeline
            .try_with_stage(Arc::new(PassStage("pass")), FinalizerFailureMode::FailOpen)
            .unwrap();
    }
    assert_eq!(
        pipeline
            .try_with_stage(Arc::new(PassStage("overflow")), FinalizerFailureMode::FailOpen)
            .unwrap_err()
            .code,
        "stage_count_exceeded"
    );
}

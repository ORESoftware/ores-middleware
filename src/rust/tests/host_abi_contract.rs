#![forbid(unsafe_code)]

use ores_middleware::{
    LocalMiddlewareHost, MIDDLEWARE_HOST_ABI_SCHEMA, MiddlewareHostBeginResult,
    MiddlewareHostCapabilities, MiddlewareHostCompletionBoundary, MiddlewareHostDescriptor,
    MiddlewareHostExecutionModel, MiddlewareHostFinishRequest, MiddlewareHostOutcome,
    MiddlewareHostRequest, MiddlewareHostResponseHeadPhase, MiddlewareHostResponseHeadRequest,
    MiddlewareStack, default_config,
};

fn local_host() -> LocalMiddlewareHost {
    let mut config = default_config("host-abi-contract");
    config.settings.rate_limit.enabled = false;
    LocalMiddlewareHost::new(MiddlewareStack::new(config).expect("middleware stack"))
}

#[tokio::test]
async fn local_host_uses_the_provider_neutral_response_lifecycle_contract() {
    let host = local_host();
    let mut request = MiddlewareHostRequest::new("GET", "/events");
    request.trusted_transport_secure = true;
    let begin = host.begin(request).await.expect("begin");
    let MiddlewareHostBeginResult::Permit {
        schema,
        session_id,
        request,
        response_headers,
        ..
    } = begin
    else {
        panic!("expected middleware permit");
    };
    assert_eq!(schema, MIDDLEWARE_HOST_ABI_SCHEMA);
    assert!(!session_id.is_empty());
    assert_eq!(request.path, "/events");
    assert!(response_headers.contains_key("x-content-type-options"));

    let available = host
        .observe_response_head(MiddlewareHostResponseHeadRequest {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            session_id: session_id.clone(),
            status: 200,
            phase: MiddlewareHostResponseHeadPhase::Available,
        })
        .expect("head available");
    let committed = host
        .observe_response_head(MiddlewareHostResponseHeadRequest {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            session_id: session_id.clone(),
            status: 200,
            phase: MiddlewareHostResponseHeadPhase::Committed,
        })
        .expect("head committed");
    assert!(committed.elapsed_ms >= available.elapsed_ms);

    let finish = host
        .finish(MiddlewareHostFinishRequest {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            session_id,
            status: 200,
            response_bytes: None,
            outcome: MiddlewareHostOutcome::Completed,
            completion_boundary: MiddlewareHostCompletionBoundary::Transport,
        })
        .await
        .expect("finish");
    assert_eq!(finish.outcome, MiddlewareHostOutcome::Completed);
    assert_eq!(finish.completion_boundary, MiddlewareHostCompletionBoundary::Transport);
    assert_eq!(finish.status, 200);
    assert!(finish.time_to_response_head_available_ms.is_some());
    assert!(finish.time_to_response_head_committed_ms.is_some());
    assert_eq!(host.active_count().expect("active count"), 0);
}

#[test]
fn descriptor_matches_capability_driven_peer_contract() {
    let config = default_config("host-descriptor-contract");
    let local = MiddlewareHostDescriptor::for_config(
        "ores-middleware/local-process",
        MiddlewareHostExecutionModel::LocalProcess,
        MiddlewareHostCapabilities::local_process(),
        &config,
    )
    .expect("local descriptor");
    let edge = MiddlewareHostDescriptor::for_config(
        "ores-middleware/cloudflare-worker",
        MiddlewareHostExecutionModel::FetchHandler,
        MiddlewareHostCapabilities {
            background_wait_until: true,
            wasm_module: true,
            ..MiddlewareHostCapabilities::local_process()
        },
        &config,
    )
    .expect("edge descriptor");
    assert_eq!(local.config_sha256, edge.config_sha256);
    assert_ne!(local.adapter_id, edge.adapter_id);
    assert_ne!(local.execution_model, edge.execution_model);
}

#[test]
fn begin_result_serializes_with_canonical_camel_case_wire_names() {
    let result = MiddlewareHostBeginResult::Permit {
        schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
        session_id: "session-1".into(),
        request_id: "request-1".into(),
        trace_id: "0123456789abcdef0123456789abcdef".into(),
        request: MiddlewareHostRequest::new("GET", "/events"),
        response_headers: Default::default(),
    };
    result.validate().expect("valid permit");
    let value = serde_json::to_value(result).expect("serialize permit");
    let object = value.as_object().expect("permit object");
    assert_eq!(object.get("decision").and_then(|value| value.as_str()), Some("permit"));
    assert!(object.contains_key("sessionId"));
    assert!(object.contains_key("requestId"));
    assert!(object.contains_key("traceId"));
    assert!(object.contains_key("responseHeaders"));
    assert!(!object.contains_key("session_id"));
}

#[test]
fn continued_request_cannot_spoof_host_trusted_transport_facts() {
    let mut original = MiddlewareHostRequest::new("GET", "/before");
    original.trusted_remote_ip = Some("203.0.113.8".into());
    original.trusted_transport_secure = true;
    original.content_length = Some(4);

    let mut continued = original
        .clone()
        .into_request_metadata()
        .expect("metadata");
    continued.path = "/after".into();
    continued.headers.insert("x-route".into(), "after".into());
    continued.remote_ip = Some("198.51.100.20".into());
    continued.transport_secure = false;
    continued.content_length = Some(999);

    let admitted = original.continued_from(continued).expect("continued request");
    assert_eq!(admitted.path, "/after");
    assert_eq!(admitted.headers.get("x-route").map(String::as_str), Some("after"));
    assert_eq!(admitted.trusted_remote_ip.as_deref(), Some("203.0.113.8"));
    assert!(admitted.trusted_transport_secure);
    assert_eq!(admitted.content_length, Some(4));
}

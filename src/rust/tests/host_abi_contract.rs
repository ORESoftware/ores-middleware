#![forbid(unsafe_code)]

use ores_middleware::{
    LocalMiddlewareHost, MIDDLEWARE_HOST_ABI_SCHEMA, MiddlewareHostBeginResult,
    MiddlewareHostDescriptor, MiddlewareHostFinishRequest, MiddlewareHostKind,
    MiddlewareHostOutcome, MiddlewareHostRequest, MiddlewareStack, default_config,
    host_abi::{
        MiddlewareHostCompletionBoundary, MiddlewareHostExecutionModel,
        MiddlewareHostResponseHeadPhase, MiddlewareHostResponseHeadRequest,
    },
};

fn local_host() -> LocalMiddlewareHost {
    let mut config = default_config("host-abi-contract");
    config.settings.rate_limit.enabled = false;
    LocalMiddlewareHost::new(MiddlewareStack::new(config).expect("middleware stack"))
}

#[tokio::test]
async fn local_host_uses_the_provider_neutral_lifecycle_contract() {
    let host = local_host();
    let mut request = MiddlewareHostRequest::new("GET", "/events");
    request.trusted_transport_secure = true;
    let begin = host.begin(request).await.expect("begin");
    let MiddlewareHostBeginResult::Permit {
        schema,
        session_id,
        response_headers,
        ..
    } = begin
    else {
        panic!("expected middleware permit");
    };
    assert_eq!(schema, MIDDLEWARE_HOST_ABI_SCHEMA);
    assert!(!session_id.is_empty());
    assert!(response_headers.contains_key("x-content-type-options"));

    host.observe_response_head(MiddlewareHostResponseHeadRequest {
        schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
        session_id: session_id.clone(),
        status: 200,
        phase: MiddlewareHostResponseHeadPhase::Available,
    })
    .expect("response head available");
    host.observe_response_head(MiddlewareHostResponseHeadRequest {
        schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
        session_id: session_id.clone(),
        status: 200,
        phase: MiddlewareHostResponseHeadPhase::Committed,
    })
    .expect("response head committed");

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
    assert_eq!(finish.status, 200);
    assert!(finish.time_to_response_head_available_ms.is_some());
    assert!(finish.time_to_response_head_committed_ms.is_some());
    assert_eq!(host.active_count().expect("active count"), 0);
}

#[test]
fn local_and_edge_descriptors_share_config_identity_but_not_adapter_capabilities() {
    let config = default_config("host-descriptor-contract");
    let local = MiddlewareHostDescriptor::for_config(MiddlewareHostKind::LocalProcess, &config)
        .expect("local descriptor");
    let edge = MiddlewareHostDescriptor::for_config(MiddlewareHostKind::CloudflareWorker, &config)
        .expect("edge descriptor");
    assert_eq!(local.config_sha256, edge.config_sha256);
    assert_ne!(local.adapter_id, edge.adapter_id);
    assert_eq!(
        local.execution_model,
        MiddlewareHostExecutionModel::LocalProcess
    );
    assert_eq!(
        edge.execution_model,
        MiddlewareHostExecutionModel::FetchHandler
    );
    assert!(local.capabilities.response_head_commit_observation);
    assert!(!edge.capabilities.response_head_commit_observation);
}

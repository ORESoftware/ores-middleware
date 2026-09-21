#![forbid(unsafe_code)]

use ores_middleware::{
    default_config, LocalMiddlewareHost, MiddlewareHostBeginResult, MiddlewareHostDescriptor,
    MiddlewareHostFinishRequest, MiddlewareHostKind, MiddlewareHostOutcome, MiddlewareHostRequest,
    MiddlewareStack, MIDDLEWARE_HOST_ABI_SCHEMA,
};

fn local_host() -> LocalMiddlewareHost {
    let mut config = default_config("host-abi-contract");
    config.settings.rate_limit.enabled = false;
    LocalMiddlewareHost::new(MiddlewareStack::new(config).expect("middleware stack"))
}

#[tokio::test]
async fn local_host_uses_the_provider_neutral_begin_finish_contract() {
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

    let finish = host
        .finish(MiddlewareHostFinishRequest {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            session_id,
            status: 200,
            response_bytes: None,
            outcome: MiddlewareHostOutcome::Completed,
        })
        .await
        .expect("finish");
    assert_eq!(finish.outcome, MiddlewareHostOutcome::Completed);
    assert_eq!(host.active_count().expect("active count"), 0);
}

#[test]
fn local_and_edge_descriptors_share_config_identity_but_not_host_identity() {
    let config = default_config("host-descriptor-contract");
    let local = MiddlewareHostDescriptor::for_config(MiddlewareHostKind::LocalProcess, &config)
        .expect("local descriptor");
    let edge = MiddlewareHostDescriptor::for_config(MiddlewareHostKind::CloudflareWorker, &config)
        .expect("edge descriptor");
    assert_eq!(local.config_sha256, edge.config_sha256);
    assert_ne!(local.host_kind, edge.host_kind);
}

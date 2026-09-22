#![forbid(unsafe_code)]

use ores_middleware::{
    edge_minimal_middleware_fn, EdgeMinimalCallbackArgs, EdgeMinimalDecision,
    EdgeMinimalMiddleware, IntegrationError, MiddlewareExecutionProfile, MiddlewareFetchRequest,
};

#[test]
fn portable_provider_api_is_importable_from_crate_root() {
    assert!(MiddlewareExecutionProfile::EdgeMinimal.is_p2_compatible());

    let middleware = edge_minimal_middleware_fn(|args: EdgeMinimalCallbackArgs| async move {
        Ok::<_, IntegrationError>(EdgeMinimalDecision::Continue(args.request))
    });

    fn assert_edge_minimal<T: EdgeMinimalMiddleware>(_middleware: &T) {}
    assert_edge_minimal(&middleware);

    let request = MiddlewareFetchRequest::get("https://example.invalid/health");
    assert_eq!(request.method, "GET");
}

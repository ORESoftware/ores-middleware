use std::collections::BTreeMap;

use crate::{ActiveRequest, MiddlewareStack};

/// Compute the response headers that belong to an admitted request without
/// consuming/finalizing that request.
///
/// Streaming hosts need these headers before they send the HTTP response head,
/// while `MiddlewareStack::finish` must remain deferred until the stream has
/// ended, timed out, or disconnected so telemetry and cleanup describe the real
/// request lifetime.
#[must_use]
pub fn response_headers(stack: &MiddlewareStack, active: &ActiveRequest) -> BTreeMap<String, String> {
    response_headers_from_context(stack, &active.context.request_id)
}

/// Pure internal projection. Keep the externally supported streaming seam tied
/// to an admitted `ActiveRequest` so callers cannot manufacture lifecycle
/// headers for a request that never crossed middleware admission.
#[must_use]
pub(crate) fn response_headers_from_context(
    stack: &MiddlewareStack,
    request_id: &str,
) -> BTreeMap<String, String> {
    let settings = &stack.config().settings;
    let security = &settings.security_headers;
    let request_id_header = (
        settings.request_id_header.clone(),
        request_id.to_owned(),
    );
    let security_headers = security.enabled.then(|| {
        [
            ("x-content-type-options".to_owned(), "nosniff".to_owned()),
            ("x-frame-options".to_owned(), security.frame_options.clone()),
            (
                "referrer-policy".to_owned(),
                "strict-origin-when-cross-origin".to_owned(),
            ),
            (
                "strict-transport-security".to_owned(),
                format!(
                    "max-age={}; includeSubDomains",
                    security.hsts_max_age_seconds
                ),
            ),
        ]
    });
    let csp_header = security
        .enabled
        .then(|| security.content_security_policy.clone())
        .flatten()
        .map(|csp| ("content-security-policy".to_owned(), csp));

    std::iter::once(request_id_header)
        .chain(security_headers.into_iter().flatten())
        .chain(csp_header)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RequestMetadata, default_config};

    #[test]
    fn headers_are_available_without_finishing_request() {
        let stack = MiddlewareStack::new(default_config("streaming-response-test"))
            .expect("middleware stack");
        let headers = response_headers_from_context(&stack, "req-stream-1");
        assert_eq!(
            headers
                .get(&stack.config().settings.request_id_header)
                .map(String::as_str),
            Some("req-stream-1")
        );
        assert_eq!(
            headers.get("x-content-type-options").map(String::as_str),
            Some("nosniff")
        );
    }

    #[tokio::test]
    async fn projected_headers_match_the_existing_finish_contract() {
        // This is a response-projection parity test, not a rate-limiter
        // integration test. Keep every unrelated admission dependency explicit
        // so a missing external limiter cannot prevent us from reaching finish().
        let mut config = default_config("streaming-response-parity");
        config.settings.rate_limit.enabled = false;
        let stack = MiddlewareStack::new(config).expect("middleware stack");
        let request_id_header = stack.config().settings.request_id_header.clone();
        let active = stack
            .begin(RequestMetadata {
                method: "GET".into(),
                path: "/stream".into(),
                headers: BTreeMap::from([(request_id_header, "req-stream-parity".into())]),
                remote_ip: None,
                content_length: None,
                // Exercise the ordinary admitted production path. The parity
                // assertion is about response projection, not HTTPS rejection.
                transport_secure: true,
            })
            .await
            .expect("request admission");

        let projected = response_headers(&stack, &active);
        let finalized = stack.finish(active, 200, None).await;

        assert_eq!(projected, finalized);
    }
}

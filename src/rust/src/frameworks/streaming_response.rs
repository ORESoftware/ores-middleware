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

/// Pure projection used by streaming adapters and tests.
#[must_use]
pub fn response_headers_from_context(
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
    use crate::default_config;

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
}

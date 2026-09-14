use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
};

use crate::stage::{
    MiddlewareStageHandler, StageDecision, StageInput, StageRejection, StageResponse,
};

#[derive(Debug, Clone, Default)]
pub struct CorsPolicy {
    pub allowed_origins: BTreeSet<String>,
    pub allowed_methods: BTreeSet<String>,
    pub allowed_headers: BTreeSet<String>,
    pub allow_credentials: bool,
    pub max_age_seconds: Option<u64>,
}

impl CorsPolicy {
    fn origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.contains(origin)
    }

    fn method_allowed(&self, method: &str) -> bool {
        self.allowed_methods
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(method))
    }

    fn header_allowed(&self, header: &str) -> bool {
        self.allowed_headers
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(header))
    }
}

pub struct CorsStage {
    policy: CorsPolicy,
}

impl CorsStage {
    pub fn new(policy: CorsPolicy) -> Self {
        Self { policy }
    }
}

impl MiddlewareStageHandler for CorsStage {
    fn name(&self) -> &'static str {
        "cors"
    }

    fn request<'a>(
        &'a self,
        input: StageInput,
    ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
        Box::pin(async move {
            let Some(origin) = header(&input.request.headers, "origin") else {
                return StageDecision::Continue(Box::new(input));
            };
            if !self.policy.origin_allowed(origin) {
                return StageDecision::Reject(StageRejection::new(
                    403,
                    "cors_origin_denied",
                    "request origin is not allowed",
                ));
            }

            let requested_method = header(
                &input.request.headers,
                "access-control-request-method",
            );
            let is_preflight = input.request.method.eq_ignore_ascii_case("OPTIONS")
                && requested_method.is_some();
            if !is_preflight {
                return StageDecision::Continue(Box::new(input));
            }

            let requested_method = requested_method.unwrap_or_default();
            if !self.policy.method_allowed(requested_method) {
                return StageDecision::Reject(StageRejection::new(
                    403,
                    "cors_method_denied",
                    "requested CORS method is not allowed",
                ));
            }

            let requested_headers = header(
                &input.request.headers,
                "access-control-request-headers",
            )
            .map(parse_comma_list)
            .unwrap_or_default();
            if requested_headers
                .iter()
                .any(|name| !self.policy.header_allowed(name))
            {
                return StageDecision::Reject(StageRejection::new(
                    403,
                    "cors_header_denied",
                    "requested CORS header is not allowed",
                ));
            }

            let mut response = StageResponse::empty(204);
            apply_cors_headers(&self.policy, origin, &mut response.headers);
            response.headers.insert(
                "access-control-allow-methods".into(),
                self.policy
                    .allowed_methods
                    .iter()
                    .map(|method| method.to_ascii_uppercase())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            if !self.policy.allowed_headers.is_empty() {
                response.headers.insert(
                    "access-control-allow-headers".into(),
                    self.policy
                        .allowed_headers
                        .iter()
                        .map(|name| name.to_ascii_lowercase())
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
            if let Some(max_age) = self.policy.max_age_seconds {
                response
                    .headers
                    .insert("access-control-max-age".into(), max_age.to_string());
            }
            merge_vary(
                &mut response.headers,
                &[
                    "origin",
                    "access-control-request-method",
                    "access-control-request-headers",
                ],
            );
            StageDecision::Respond(response)
        })
    }

    fn response<'a>(
        &'a self,
        input: &'a StageInput,
        mut response: StageResponse,
    ) -> Pin<Box<dyn Future<Output = StageResponse> + Send + 'a>> {
        Box::pin(async move {
            if let Some(origin) = header(&input.request.headers, "origin")
                && self.policy.origin_allowed(origin)
            {
                apply_cors_headers(&self.policy, origin, &mut response.headers);
            }
            response
        })
    }
}

#[derive(Debug, Clone)]
pub struct CsrfPolicy {
    pub trusted_origins: BTreeSet<String>,
    pub token_cookie_name: String,
    pub token_header_name: String,
    pub protected_cookie_names: BTreeSet<String>,
    pub unsafe_methods: BTreeSet<String>,
}

impl Default for CsrfPolicy {
    fn default() -> Self {
        Self {
            trusted_origins: BTreeSet::new(),
            token_cookie_name: "ores_csrf".into(),
            token_header_name: "x-ores-csrf-token".into(),
            protected_cookie_names: BTreeSet::new(),
            unsafe_methods: ["POST", "PUT", "PATCH", "DELETE"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        }
    }
}

pub struct CsrfStage {
    policy: CsrfPolicy,
}

impl CsrfStage {
    pub fn new(policy: CsrfPolicy) -> Self {
        Self { policy }
    }

    fn is_unsafe_method(&self, method: &str) -> bool {
        self.policy
            .unsafe_methods
            .iter()
            .any(|unsafe_method| unsafe_method.eq_ignore_ascii_case(method))
    }
}

impl MiddlewareStageHandler for CsrfStage {
    fn name(&self) -> &'static str {
        "csrf"
    }

    fn request<'a>(
        &'a self,
        input: StageInput,
    ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
        Box::pin(async move {
            if !self.is_unsafe_method(&input.request.method) {
                return StageDecision::Continue(Box::new(input));
            }

            let cookies = header(&input.request.headers, "cookie")
                .map(parse_cookie_header)
                .unwrap_or_default();
            if !uses_protected_cookie(&self.policy, &cookies) {
                return StageDecision::Continue(Box::new(input));
            }

            let Some(origin) = header(&input.request.headers, "origin") else {
                return StageDecision::Reject(StageRejection::new(
                    403,
                    "csrf_origin_required",
                    "unsafe cookie-authenticated requests require an origin",
                ));
            };
            if !self.policy.trusted_origins.contains(origin) {
                return StageDecision::Reject(StageRejection::new(
                    403,
                    "csrf_origin_denied",
                    "request origin is not trusted",
                ));
            }

            let cookie_token = cookies.get(&self.policy.token_cookie_name);
            let header_token = header(&input.request.headers, &self.policy.token_header_name);
            if cookie_token.is_none()
                || header_token.is_none()
                || cookie_token.map(String::as_str) != header_token
                || header_token.is_some_and(str::is_empty)
            {
                return StageDecision::Reject(StageRejection::new(
                    403,
                    "csrf_token_invalid",
                    "CSRF token validation failed",
                ));
            }

            StageDecision::Continue(Box::new(input))
        })
    }
}

fn uses_protected_cookie(policy: &CsrfPolicy, cookies: &BTreeMap<String, String>) -> bool {
    if cookies.is_empty() {
        return false;
    }
    if policy.protected_cookie_names.is_empty() {
        return true;
    }
    policy
        .protected_cookie_names
        .iter()
        .any(|name| cookies.contains_key(name))
}

fn header<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn parse_comma_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn parse_cookie_header(value: &str) -> BTreeMap<String, String> {
    value
        .split(';')
        .filter_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            let name = name.trim();
            (!name.is_empty()).then(|| (name.to_owned(), value.trim().to_owned()))
        })
        .collect()
}

fn apply_cors_headers(
    policy: &CorsPolicy,
    origin: &str,
    headers: &mut BTreeMap<String, String>,
) {
    headers.insert("access-control-allow-origin".into(), origin.to_owned());
    if policy.allow_credentials {
        headers.insert("access-control-allow-credentials".into(), "true".into());
    }
    merge_vary(headers, &["origin"]);
}

fn merge_vary(headers: &mut BTreeMap<String, String>, names: &[&str]) {
    let mut values: BTreeMap<String, String> = headers
        .get("vary")
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| (value.to_ascii_lowercase(), value.to_owned()))
                .collect()
        })
        .unwrap_or_default();
    for name in names {
        values
            .entry(name.to_ascii_lowercase())
            .or_insert_with(|| (*name).to_owned());
    }
    if !values.is_empty() {
        headers.insert(
            "vary".into(),
            values.into_values().collect::<Vec<_>>().join(", "),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{context::RequestContext, integrations::RequestMetadata, stage::StagePipeline};

    fn request(method: &str, headers: &[(&str, &str)]) -> StageInput {
        StageInput::new(
            RequestMetadata {
                method: method.into(),
                path: "/widgets".into(),
                headers: headers
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                    .collect(),
                remote_ip: None,
                content_length: None,
                transport_secure: true,
            },
            RequestContext {
                request_id: "req-1".into(),
                trace_id: "0123456789abcdef0123456789abcdef".into(),
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

    fn cors_policy() -> CorsPolicy {
        CorsPolicy {
            allowed_origins: ["https://app.example.test".to_owned()].into_iter().collect(),
            allowed_methods: ["GET".to_owned(), "POST".to_owned()].into_iter().collect(),
            allowed_headers: ["content-type".to_owned(), "x-ores-request-id".to_owned()]
                .into_iter()
                .collect(),
            allow_credentials: true,
            max_age_seconds: Some(600),
        }
    }

    #[tokio::test]
    async fn cors_preflight_short_circuits_with_exact_origin_and_vary() {
        let pipeline = StagePipeline::new().with_stage(Arc::new(CorsStage::new(cors_policy())));
        let response = pipeline
            .execute(
                request(
                    "OPTIONS",
                    &[
                        ("origin", "https://app.example.test"),
                        ("access-control-request-method", "POST"),
                        (
                            "access-control-request-headers",
                            "Content-Type, X-ORES-Request-ID",
                        ),
                    ],
                ),
                |_| async { panic!("preflight must not reach application handler") },
            )
            .await;
        assert_eq!(response.status, 204);
        assert_eq!(
            response
                .headers
                .get("access-control-allow-origin")
                .map(String::as_str),
            Some("https://app.example.test")
        );
        let vary = response.headers.get("vary").expect("vary header");
        assert!(vary.contains("origin"));
        assert!(vary.contains("access-control-request-method"));
        assert!(vary.contains("access-control-request-headers"));
    }

    #[tokio::test]
    async fn cors_rejects_untrusted_origin_before_handler() {
        let pipeline = StagePipeline::new().with_stage(Arc::new(CorsStage::new(cors_policy())));
        let response = pipeline
            .execute(
                request("GET", &[("origin", "https://evil.example")]),
                |_| async { panic!("untrusted origin must not reach application handler") },
            )
            .await;
        assert_eq!(response.status, 403);
    }

    #[tokio::test]
    async fn cors_finalizer_marks_normal_responses() {
        let pipeline = StagePipeline::new().with_stage(Arc::new(CorsStage::new(cors_policy())));
        let response = pipeline
            .execute(
                request("GET", &[("origin", "https://app.example.test")]),
                |_| async { StageResponse::empty(200) },
            )
            .await;
        assert_eq!(
            response
                .headers
                .get("access-control-allow-origin")
                .map(String::as_str),
            Some("https://app.example.test")
        );
        assert_eq!(
            response
                .headers
                .get("access-control-allow-credentials")
                .map(String::as_str),
            Some("true")
        );
    }

    fn csrf_policy() -> CsrfPolicy {
        CsrfPolicy {
            trusted_origins: ["https://app.example.test".to_owned()].into_iter().collect(),
            token_cookie_name: "ores_csrf".into(),
            token_header_name: "x-ores-csrf-token".into(),
            protected_cookie_names: ["session".to_owned()].into_iter().collect(),
            ..CsrfPolicy::default()
        }
    }

    #[tokio::test]
    async fn csrf_does_not_block_bearer_only_mutations() {
        let pipeline = StagePipeline::new().with_stage(Arc::new(CsrfStage::new(csrf_policy())));
        let response = pipeline
            .execute(
                request("POST", &[("authorization", "Bearer test")]),
                |_| async { StageResponse::empty(201) },
            )
            .await;
        assert_eq!(response.status, 201);
    }

    #[tokio::test]
    async fn csrf_rejects_cookie_authenticated_mutation_without_token() {
        let pipeline = StagePipeline::new().with_stage(Arc::new(CsrfStage::new(csrf_policy())));
        let response = pipeline
            .execute(
                request(
                    "POST",
                    &[
                        ("cookie", "session=s1; ores_csrf=t1"),
                        ("origin", "https://app.example.test"),
                    ],
                ),
                |_| async { panic!("invalid CSRF request must not reach application handler") },
            )
            .await;
        assert_eq!(response.status, 403);
    }

    #[tokio::test]
    async fn csrf_accepts_matching_double_submit_token_from_trusted_origin() {
        let pipeline = StagePipeline::new().with_stage(Arc::new(CsrfStage::new(csrf_policy())));
        let response = pipeline
            .execute(
                request(
                    "POST",
                    &[
                        ("cookie", "session=s1; ores_csrf=t1"),
                        ("origin", "https://app.example.test"),
                        ("x-ores-csrf-token", "t1"),
                    ],
                ),
                |_| async { StageResponse::empty(204) },
            )
            .await;
        assert_eq!(response.status, 204);
    }
}

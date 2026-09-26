use std::collections::{BTreeMap, BTreeSet};

use ores_middleware::{
    CorsPolicy, CorsStage, CsrfPolicy, CsrfStage, MiddlewareStageHandler, RequestContext,
    RequestMetadata, StageDecision, StageInput,
    frameworks::ingress::{
        BrowserAdmissionError, BrowserSecurityPolicy, CERTIFIED_HTTP_INGRESS_ADAPTERS,
    },
};

fn context() -> RequestContext {
    return RequestContext {
        request_id: "request-1".to_owned(),
        trace_id: "0123456789abcdef0123456789abcdef".to_owned(),
        span_id: None,
        tenant_id: None,
        user_id: None,
        locale: None,
        started_at_unix_ms: 0,
        deadline_unix_ms: None,
        baggage: BTreeMap::new(),
    };
}

fn input(method: &str, headers: &[(&str, &str)]) -> StageInput {
    let headers = headers
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    return StageInput::new(
        RequestMetadata {
            method: method.to_owned(),
            path: "/resource".to_owned(),
            headers,
            remote_ip: Some("127.0.0.1".to_owned()),
            content_length: None,
            transport_secure: true,
        },
        context(),
    );
}

fn cors_stage() -> CorsStage {
    return CorsStage::new(CorsPolicy {
        allowed_origins: BTreeSet::from(["https://app.example.com".to_owned()]),
        allowed_methods: BTreeSet::from(["GET".to_owned(), "POST".to_owned()]),
        allowed_headers: BTreeSet::from(["content-type".to_owned(), "x-ores-csrf-token".to_owned()]),
        allow_credentials: true,
        max_age_seconds: Some(600),
    });
}

fn csrf_stage() -> CsrfStage {
    return CsrfStage::new(CsrfPolicy {
        trusted_origins: BTreeSet::from(["https://app.example.com".to_owned()]),
        token_cookie_name: "ores_csrf".to_owned(),
        token_header_name: "x-ores-csrf-token".to_owned(),
        protected_cookie_names: BTreeSet::from(["session".to_owned()]),
        unsafe_methods: BTreeSet::from([
            "POST".to_owned(),
            "PUT".to_owned(),
            "PATCH".to_owned(),
            "DELETE".to_owned(),
        ]),
    });
}

#[tokio::test]
async fn real_cors_stage_denies_untrusted_origin_for_every_certified_adapter() {
    for _adapter in CERTIFIED_HTTP_INGRESS_ADAPTERS {
        let decision = cors_stage()
            .request(input("GET", &[("origin", "https://evil.example")]))
            .await;
        match decision {
            StageDecision::Reject(rejection) => {
                assert_eq!(rejection.status, 403);
                assert_eq!(rejection.code, "cors_origin_denied");
                assert!(!rejection.message.contains("internal"));
            }
            _ => panic!("untrusted browser origin must be rejected"),
        }
    }
}

#[tokio::test]
async fn real_csrf_stage_requires_double_submit_token_for_cookie_mutations() {
    for _adapter in CERTIFIED_HTTP_INGRESS_ADAPTERS {
        let missing = csrf_stage()
            .request(input(
                "POST",
                &[
                    ("origin", "https://app.example.com"),
                    ("cookie", "session=opaque; ores_csrf=token-123"),
                ],
            ))
            .await;
        match missing {
            StageDecision::Reject(rejection) => {
                assert_eq!(rejection.status, 403);
                assert_eq!(rejection.code, "csrf_token_invalid");
            }
            _ => panic!("cookie-authenticated mutation without CSRF header must fail"),
        }

        let admitted = csrf_stage()
            .request(input(
                "POST",
                &[
                    ("origin", "https://app.example.com"),
                    ("cookie", "session=opaque; ores_csrf=token-123"),
                    ("x-ores-csrf-token", "token-123"),
                ],
            ))
            .await;
        assert!(matches!(admitted, StageDecision::Continue(_)));
    }
}

#[test]
fn secure_cookie_policy_is_identical_for_every_certified_adapter() {
    let policy = BrowserSecurityPolicy {
        allowed_origins: vec!["https://app.example.com".to_owned()],
        allow_credentials: true,
        require_csrf_for_cookie_mutations: true,
        require_secure_session_cookies: true,
    };

    for adapter in CERTIFIED_HTTP_INGRESS_ADAPTERS {
        assert_eq!(
            policy.admit_session_set_cookie(
                *adapter,
                "session=opaque; Path=/; Secure; HttpOnly; SameSite=Strict"
            ),
            Ok(())
        );
        assert_eq!(
            policy.admit_session_set_cookie(*adapter, "session=opaque; Path=/; HttpOnly"),
            Err(BrowserAdmissionError::InsecureSessionCookie)
        );
    }
}

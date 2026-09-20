use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::Arc,
};

use ores_middleware::{
    AuthDecision, AuthVerifier, IntegrationError, MiddlewareStack, RequestMetadata, default_config,
};

const PROVIDER_SECRET_CODE: &str = "provider_secret_code";
const PROVIDER_SECRET_MESSAGE: &str = "sdk diagnostic contains bearer=super-secret-token";

struct SecretFailingAuth;

impl AuthVerifier for SecretFailingAuth {
    fn verify<'a>(
        &'a self,
        _request: &'a RequestMetadata,
    ) -> Pin<Box<dyn Future<Output = Result<AuthDecision, IntegrationError>> + Send + 'a>> {
        Box::pin(async {
            Err(IntegrationError {
                code: PROVIDER_SECRET_CODE,
                message: PROVIDER_SECRET_MESSAGE.to_owned(),
            })
        })
    }
}

#[tokio::test]
async fn middleware_stack_never_exposes_provider_error_code_or_message() {
    let mut config = default_config("auth-error-sanitization-test");
    config.settings.tls.mode = "in-process".into();
    config.settings.tls.trusted_proxy_cidrs.clear();
    config.settings.rate_limit.enabled = false;

    let stack = MiddlewareStack::new(config)
        .expect("valid middleware config")
        .with_auth_verifier(Arc::new(SecretFailingAuth));

    let request = RequestMetadata {
        method: "GET".into(),
        path: "/private".into(),
        headers: BTreeMap::from([("x-request-id".into(), "auth-secret-probe".into())]),
        remote_ip: Some("203.0.113.9".into()),
        content_length: None,
        transport_secure: true,
    };

    let error = match stack.begin(request).await {
        Ok(_) => panic!("failing authentication must not create an active request"),
        Err(error) => error,
    };

    assert_eq!(error.status, 401);
    assert_eq!(error.code, "authentication_failed");
    assert_eq!(error.message, "authentication failed");
    assert!(!error.message.contains(PROVIDER_SECRET_CODE));
    assert!(!error.message.contains(PROVIDER_SECRET_MESSAGE));
    assert_eq!(
        error.headers.get("x-request-id").map(String::as_str),
        Some("auth-secret-probe")
    );
}

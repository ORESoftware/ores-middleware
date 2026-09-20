//! `IntegrationError` is returned from this crate's public surface
//! (`MiddlewareStack::with_rate_limit_hmac_key`, `AuthVerifier`, `SyncObserver`,
//! `RateLimiter`), so consumers must be able to treat it as an ordinary error:
//! propagate it with `?`, box it, and print it. These tests pin that contract.

use ores_middleware::{IntegrationError, MiddlewareStack, default_config};

fn sample() -> IntegrationError {
    IntegrationError {
        code: "rate_limit_hmac_key_too_short",
        message: "rate-limit HMAC keys must contain at least 32 bytes".to_owned(),
    }
}

#[test]
fn display_reports_message_then_code() {
    assert_eq!(
        sample().to_string(),
        "rate-limit HMAC keys must contain at least 32 bytes (rate_limit_hmac_key_too_short)"
    );
}

#[test]
fn error_source_is_absent_because_the_type_is_a_leaf() {
    let error = sample();
    assert!(std::error::Error::source(&error).is_none());
}

#[test]
fn integration_error_converts_into_boxed_std_error() {
    // This is the exact conversion a consumer's `?` performs. It only compiles
    // because `IntegrationError: std::error::Error + Send + Sync + 'static`.
    fn propagate() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err(sample())?;
        Ok(())
    }

    let message = propagate().unwrap_err().to_string();
    assert!(message.contains("rate_limit_hmac_key_too_short"));
}

#[test]
fn a_rejected_hmac_key_propagates_through_the_question_mark_operator() {
    // The regression this pins: `with_rate_limit_hmac_key` returns
    // `Result<Self, IntegrationError>`, and downstream binaries apply `?` to it
    // inside a `Result<_, Box<dyn std::error::Error>>` function.
    fn install() -> Result<(), Box<dyn std::error::Error>> {
        MiddlewareStack::new(default_config("integration-error-contract"))
            .map_err(|issues| format!("config rejected: {issues:?}"))?
            .with_rate_limit_hmac_key(b"too-short")?;
        Ok(())
    }

    let Err(error) = install() else {
        panic!("a 9-byte key must be rejected");
    };
    assert!(error.to_string().contains("rate_limit_hmac_key_too_short"));
}

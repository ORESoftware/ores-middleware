//! Audit-only Axum installation that cannot consume or grant a rate-limit quota.
//!
//! Use this adapter for staged fleet rollout while existing service-owned
//! limiters remain authoritative. Enabling shared enforcement requires a
//! separate reviewed change to the normal `axum::install_from_env` boundary.

use std::sync::Arc;

use axum::Router;

use crate::{
    BootstrapError, MiddlewareConfig, MiddlewareStack, ValidationIssue, config_from_env,
};

/// Install the shared request lifecycle while forcibly disabling its rate-limit
/// decision. Other validated middleware settings still come from the standard
/// environment contract.
pub fn install_from_env(
    router: Router,
    service_name: impl Into<String>,
) -> Result<Router, BootstrapError> {
    install_with_config(router, config_from_env(service_name)?)
}

/// Install an explicitly resolved configuration in audit-only rate-limit mode.
///
/// This is the preferred entry point for applications that resolve flags and
/// environment variables at their own argv boundary before constructing the
/// middleware configuration.
pub fn install_with_config(
    router: Router,
    config: MiddlewareConfig,
) -> Result<Router, BootstrapError> {
    let stack = MiddlewareStack::new(audit_config(config)).map_err(invalid_config)?;
    Ok(super::axum::install(router, Arc::new(stack)))
}

fn audit_config(mut config: MiddlewareConfig) -> MiddlewareConfig {
    config.settings.rate_limit.enabled = false;
    config
}

fn invalid_config(issues: Vec<ValidationIssue>) -> BootstrapError {
    BootstrapError {
        variable: None,
        code: "invalid_config",
        message: issues
            .iter()
            .map(|issue| format!("{}:{}", issue.path, issue.code))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeEnvironment, default_config};

    #[test]
    fn audit_configuration_disables_rate_limiting_even_in_production() {
        let mut config = default_config("audit-fixture");
        config.environment = RuntimeEnvironment::Production;
        config.settings.rate_limit.enabled = true;
        let config = audit_config(config);
        assert!(!config.settings.rate_limit.enabled);
    }

    #[test]
    fn audit_adapter_builds_without_a_rate_limit_hmac_secret() {
        let mut config = default_config("audit-fixture");
        config.settings.rate_limit.enabled = true;
        assert!(install_with_config(Router::new(), config).is_ok());
    }
}

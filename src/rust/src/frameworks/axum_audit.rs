//! Audit-only Axum installation that cannot consume or grant a rate-limit quota.
//!
//! Use this adapter for staged fleet rollout while existing service-owned
//! limiters remain authoritative. Enabling shared enforcement requires a
//! separate reviewed change to the normal `axum::install_from_env` boundary.

use std::sync::Arc;

use axum::Router;

use crate::{
    BootstrapError, MiddlewareConfig, MiddlewareStack, ValidationIssue, admit_server_stack,
    config_from_env,
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

/// Audit-only counterpart to the normal Axum manifest gate.
///
/// The embedded `.ores-mw.toml` must select the expected enabled server stack target before the
/// existing environment configuration is admitted. This keeps staged/audit deployments from
/// bypassing the same repository-local middleware orchestration contract used by enforcement mode.
pub fn install_from_env_with_manifest(
    router: Router,
    service_name: impl Into<String>,
    manifest_source: &str,
    target_name: Option<&str>,
    expected_stack_config: &str,
) -> Result<Router, BootstrapError> {
    admit_server_stack(manifest_source, target_name, expected_stack_config).map_err(|error| {
        BootstrapError {
            variable: None,
            code: error.code(),
            message: ".ores-mw.toml runtime target admission failed".into(),
        }
    })?;
    install_from_env(router, service_name)
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

    const MANIFEST: &str = r#"
schema_version = 1
repository_mode = "server-only"
default_target = "api"

[[targets]]
name = "api"
role = "server"
roots = ["src"]
middleware = "stack"
stack_config = "config/middleware.json"
"#;

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

    #[test]
    fn audit_adapter_rejects_manifest_drift_before_env_bootstrap() {
        let error = install_from_env_with_manifest(
            Router::new(),
            "audit-fixture",
            MANIFEST,
            None,
            "config/other.json",
        )
        .expect_err("audit-only mode must not bypass manifest admission");
        assert_eq!(error.code, "runtime_stack_config_mismatch");
        assert!(error.variable.is_none());
        assert!(!error.message.contains("config/other.json"));
    }
}

use std::{env, fmt, str::FromStr};

use crate::{
    default_config, validate_config, MiddlewareConfig, MiddlewareStack, RuntimeEnvironment,
    ValidationIssue,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapError {
    pub variable: Option<String>,
    pub code: &'static str,
    pub message: String,
}

impl BootstrapError {
    fn variable(
        variable: impl Into<String>,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            variable: Some(variable.into()),
            code,
            message: message.into(),
        }
    }

    fn config(issues: Vec<ValidationIssue>) -> Self {
        let message = issues
            .iter()
            .map(|issue| format!("{}:{}", issue.path, issue.code))
            .collect::<Vec<_>>()
            .join(", ");
        Self {
            variable: None,
            code: "invalid_config",
            message,
        }
    }
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.variable {
            Some(variable) => write!(formatter, "{variable}: {} ({})", self.message, self.code),
            None => write!(formatter, "{} ({})", self.message, self.code),
        }
    }
}

impl std::error::Error for BootstrapError {}

pub fn config_from_env(
    service_name: impl Into<String>,
) -> Result<MiddlewareConfig, BootstrapError> {
    config_from_lookup(service_name.into(), |name| env::var(name).ok())
}

pub fn stack_from_env(
    service_name: impl Into<String>,
) -> Result<MiddlewareStack, BootstrapError> {
    let config = config_from_env(service_name)?;
    MiddlewareStack::new(config).map_err(BootstrapError::config)
}

fn config_from_lookup<F>(
    service_name: String,
    lookup: F,
) -> Result<MiddlewareConfig, BootstrapError>
where
    F: Fn(&str) -> Option<String>,
{
    let mut config = default_config(service_name);

    let environment_value = first_value(
        &lookup,
        &["ORES_MIDDLEWARE_ENV", "APP_ENV", "RUST_ENV"],
    )
    .unwrap_or_else(|| "development".to_owned());
    config.environment = parse_environment(&environment_value)?;

    let tls_mode = lookup("ORES_MIDDLEWARE_TLS_MODE");
    match tls_mode.as_deref().map(normalize) {
        None if matches!(config.environment, RuntimeEnvironment::Production) => {
            return Err(BootstrapError::variable(
                "ORES_MIDDLEWARE_TLS_MODE",
                "required_in_production",
                "production must explicitly select in-process or trusted-proxy TLS",
            ));
        }
        None => {
            config.settings.tls.mode = "disabled".into();
            config.settings.tls.require_https = false;
            config.settings.tls.trusted_proxy_cidrs.clear();
        }
        Some("disabled") => {
            if matches!(config.environment, RuntimeEnvironment::Production) {
                return Err(BootstrapError::variable(
                    "ORES_MIDDLEWARE_TLS_MODE",
                    "production_forbidden",
                    "TLS cannot be disabled in production",
                ));
            }
            config.settings.tls.mode = "disabled".into();
            config.settings.tls.require_https = false;
            config.settings.tls.trusted_proxy_cidrs.clear();
        }
        Some("in-process") | Some("in_process") | Some("inprocess") => {
            config.settings.tls.mode = "in-process".into();
            config.settings.tls.require_https = true;
            config.settings.tls.trusted_proxy_cidrs.clear();
        }
        Some("trusted-proxy") | Some("trusted_proxy") | Some("proxy") => {
            config.settings.tls.mode = "trusted-proxy".into();
            config.settings.tls.require_https = true;
            config.settings.tls.trusted_proxy_cidrs = lookup(
                "ORES_MIDDLEWARE_TRUSTED_PROXY_CIDRS",
            )
            .map(|value| split_csv(&value))
            .unwrap_or_default();
            if config.settings.tls.trusted_proxy_cidrs.is_empty() {
                return Err(BootstrapError::variable(
                    "ORES_MIDDLEWARE_TRUSTED_PROXY_CIDRS",
                    "required_for_trusted_proxy",
                    "trusted-proxy mode requires one or more explicit CIDRs",
                ));
            }
        }
        Some(value) => {
            return Err(BootstrapError::variable(
                "ORES_MIDDLEWARE_TLS_MODE",
                "invalid_value",
                format!("unsupported TLS mode {value:?}"),
            ));
        }
    }

    if let Some(value) = lookup("ORES_MIDDLEWARE_TIMEOUT_MS") {
        config.settings.timeout_ms = parse_number(&value, "ORES_MIDDLEWARE_TIMEOUT_MS")?;
    }
    if let Some(value) = lookup("ORES_MIDDLEWARE_MAX_BODY_BYTES") {
        config.settings.max_body_bytes =
            parse_number(&value, "ORES_MIDDLEWARE_MAX_BODY_BYTES")?;
    }
    if let Some(value) = lookup("ORES_MIDDLEWARE_RATE_LIMIT_ENABLED") {
        config.settings.rate_limit.enabled =
            parse_bool(&value, "ORES_MIDDLEWARE_RATE_LIMIT_ENABLED")?;
    }
    if let Some(value) = lookup("ORES_MIDDLEWARE_RATE_LIMIT_CAPACITY") {
        config.settings.rate_limit.capacity =
            parse_number(&value, "ORES_MIDDLEWARE_RATE_LIMIT_CAPACITY")?;
    }
    if let Some(value) = lookup("ORES_MIDDLEWARE_RATE_LIMIT_REFILL_PER_SECOND") {
        config.settings.rate_limit.refill_per_second = parse_float(
            &value,
            "ORES_MIDDLEWARE_RATE_LIMIT_REFILL_PER_SECOND",
        )?;
    }
    if let Some(value) = lookup("ORES_MIDDLEWARE_CONTEXT_REGISTRY_MAX_ENTRIES") {
        config.settings.context_registry_max_entries = parse_number(
            &value,
            "ORES_MIDDLEWARE_CONTEXT_REGISTRY_MAX_ENTRIES",
        )?;
    }
    if let Some(value) = lookup("ORES_MIDDLEWARE_CONTEXT_REGISTRY_TTL_MS") {
        config.settings.context_registry_ttl_ms = parse_number(
            &value,
            "ORES_MIDDLEWARE_CONTEXT_REGISTRY_TTL_MS",
        )?;
    }

    let issues = validate_config(&config);
    if issues.is_empty() {
        Ok(config)
    } else {
        Err(BootstrapError::config(issues))
    }
}

fn first_value<F>(lookup: &F, names: &[&str]) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    names.iter().find_map(|name| lookup(name))
}

fn parse_environment(value: &str) -> Result<RuntimeEnvironment, BootstrapError> {
    match normalize(value) {
        "development" | "dev" | "local" => Ok(RuntimeEnvironment::Development),
        "test" | "testing" => Ok(RuntimeEnvironment::Test),
        "staging" | "stage" => Ok(RuntimeEnvironment::Staging),
        "production" | "prod" => Ok(RuntimeEnvironment::Production),
        other => Err(BootstrapError::variable(
            "ORES_MIDDLEWARE_ENV",
            "invalid_value",
            format!("unsupported runtime environment {other:?}"),
        )),
    }
}

fn parse_bool(value: &str, variable: &str) -> Result<bool, BootstrapError> {
    match normalize(value) {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => Err(BootstrapError::variable(
            variable,
            "invalid_boolean",
            format!("expected true or false, got {other:?}"),
        )),
    }
}

fn parse_number<T>(value: &str, variable: &str) -> Result<T, BootstrapError>
where
    T: FromStr,
{
    value.trim().parse::<T>().map_err(|_| {
        BootstrapError::variable(
            variable,
            "invalid_number",
            format!("expected a positive numeric value, got {value:?}"),
        )
    })
}

fn parse_float(value: &str, variable: &str) -> Result<f64, BootstrapError> {
    value.trim().parse::<f64>().map_err(|_| {
        BootstrapError::variable(
            variable,
            "invalid_number",
            format!("expected a numeric value, got {value:?}"),
        )
    })
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn normalize(value: &str) -> &str {
    value.trim()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn from(values: &[(&str, &str)]) -> Result<MiddlewareConfig, BootstrapError> {
        let values = values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<BTreeMap<_, _>>();
        config_from_lookup("test-service".into(), |name| values.get(name).cloned())
    }

    #[test]
    fn development_defaults_to_explicitly_disabled_tls() {
        let config = from(&[]).unwrap();
        assert_eq!(config.settings.tls.mode, "disabled");
        assert!(!config.settings.tls.require_https);
    }

    #[test]
    fn production_requires_an_explicit_tls_mode() {
        let error = from(&[("ORES_MIDDLEWARE_ENV", "production")]).unwrap_err();
        assert_eq!(error.code, "required_in_production");
    }

    #[test]
    fn production_rejects_disabled_tls() {
        let error = from(&[
            ("ORES_MIDDLEWARE_ENV", "production"),
            ("ORES_MIDDLEWARE_TLS_MODE", "disabled"),
        ])
        .unwrap_err();
        assert_eq!(error.code, "production_forbidden");
    }

    #[test]
    fn trusted_proxy_requires_cidrs() {
        let error = from(&[("ORES_MIDDLEWARE_TLS_MODE", "trusted-proxy")]).unwrap_err();
        assert_eq!(error.code, "required_for_trusted_proxy");
    }

    #[test]
    fn parses_trusted_proxy_and_runtime_limits() {
        let config = from(&[
            ("ORES_MIDDLEWARE_ENV", "prod"),
            ("ORES_MIDDLEWARE_TLS_MODE", "trusted-proxy"),
            (
                "ORES_MIDDLEWARE_TRUSTED_PROXY_CIDRS",
                "10.0.0.0/8, 2001:db8::/32",
            ),
            ("ORES_MIDDLEWARE_TIMEOUT_MS", "7500"),
            ("ORES_MIDDLEWARE_MAX_BODY_BYTES", "1048576"),
            ("ORES_MIDDLEWARE_RATE_LIMIT_ENABLED", "false"),
        ])
        .unwrap();
        assert!(matches!(config.environment, RuntimeEnvironment::Production));
        assert_eq!(config.settings.tls.trusted_proxy_cidrs.len(), 2);
        assert_eq!(config.settings.timeout_ms, 7_500);
        assert_eq!(config.settings.max_body_bytes, 1_048_576);
        assert!(!config.settings.rate_limit.enabled);
    }
}

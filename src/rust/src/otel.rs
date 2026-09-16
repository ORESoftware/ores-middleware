use std::{collections::BTreeMap, future::Future, sync::Arc};

use http::Uri;
use sha2::{Digest, Sha256};

use crate::RequestContext;

// Re-export the canonical Rust logger and `.ores-otel.toml` loader so downstream
// services can use ores-middleware as the single integration surface. The
// protocol/log-level/environment types remain nested under next_loggers::config
// upstream, so expose them explicitly here as part of this integration surface.
pub use next_loggers::{
    config::{OresOtelEnv, OresOtelExporterProtocol, OresOtelLogLevel},
    *,
};

#[derive(Debug)]
pub enum ServerOtelRuntimeError {
    Config(OresOtelConfigError),
    ServiceNameMismatch { expected: String, actual: String },
    UnsupportedAutoSend,
    MissingExporterEndpointEnv,
    MissingExporterEndpoint(String),
    InvalidExporterEndpoint(String),
    MissingOpenTelemetryTransport,
    Logger(LoggerError),
}

impl std::fmt::Display for ServerOtelRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(error) => write!(formatter, "invalid .ores-otel.toml runtime: {error}"),
            Self::ServiceNameMismatch { expected, actual } => write!(
                formatter,
                "resolved telemetry service_name {actual:?} does not match process service {expected:?}"
            ),
            Self::UnsupportedAutoSend => formatter.write_str(
                "logging.auto_send=true is not supported by the Rust middleware request logger",
            ),
            Self::MissingExporterEndpointEnv => formatter.write_str(
                "an OTLP exporter protocol requires exporter.endpoint_env to name the endpoint environment variable",
            ),
            Self::MissingExporterEndpoint(name) => write!(
                formatter,
                "OTLP exporter endpoint environment variable {name} is missing or blank"
            ),
            Self::InvalidExporterEndpoint(reason) => {
                write!(formatter, "invalid OTLP exporter endpoint: {reason}")
            }
            Self::MissingOpenTelemetryTransport => formatter.write_str(
                "an OTLP exporter protocol requires an explicit application-owned OpenTelemetry transport",
            ),
            Self::Logger(error) => write!(formatter, "telemetry logger error: {error}"),
        }
    }
}

impl std::error::Error for ServerOtelRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(error) => Some(error),
            Self::Logger(error) => Some(error),
            Self::ServiceNameMismatch { .. }
            | Self::UnsupportedAutoSend
            | Self::MissingExporterEndpointEnv
            | Self::MissingExporterEndpoint(_)
            | Self::InvalidExporterEndpoint(_)
            | Self::MissingOpenTelemetryTransport => None,
        }
    }
}

impl From<OresOtelConfigError> for ServerOtelRuntimeError {
    fn from(error: OresOtelConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<LoggerError> for ServerOtelRuntimeError {
    fn from(error: LoggerError) -> Self {
        Self::Logger(error)
    }
}

#[derive(Clone)]
pub struct ServerOtelRuntime {
    pub config: ResolvedOresOtelConfig,
    pub logger: Option<Logger>,
    pub exporter_endpoint: Option<String>,
}

impl ServerOtelRuntime {
    pub fn close(&self) -> Result<(), ServerOtelRuntimeError> {
        if let Some(logger) = &self.logger {
            logger.close()?;
        }
        Ok(())
    }
}

fn log_level(level: OresOtelLogLevel) -> LogLevel {
    match level {
        OresOtelLogLevel::Trace => LogLevel::Trace,
        OresOtelLogLevel::Debug => LogLevel::Debug,
        OresOtelLogLevel::Info => LogLevel::Info,
        OresOtelLogLevel::Warn => LogLevel::Warn,
        OresOtelLogLevel::Error => LogLevel::Error,
        OresOtelLogLevel::Fatal => LogLevel::Fatal,
    }
}

fn validate_exporter_endpoint(raw: &str) -> Result<(), ServerOtelRuntimeError> {
    if raw.is_empty() || raw.chars().any(char::is_control) || raw.chars().any(char::is_whitespace) {
        return Err(ServerOtelRuntimeError::InvalidExporterEndpoint(
            "endpoint must be non-empty and contain no whitespace/control characters".into(),
        ));
    }
    if raw.contains('#') {
        return Err(ServerOtelRuntimeError::InvalidExporterEndpoint(
            "fragments are forbidden".into(),
        ));
    }
    let uri = raw.parse::<Uri>().map_err(|_| {
        ServerOtelRuntimeError::InvalidExporterEndpoint("expected an absolute URI".into())
    })?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return Err(ServerOtelRuntimeError::InvalidExporterEndpoint(
            "only http and https schemes are supported".into(),
        ));
    }
    let authority = uri.authority().ok_or_else(|| {
        ServerOtelRuntimeError::InvalidExporterEndpoint("a host authority is required".into())
    })?;
    if authority.as_str().contains('@') {
        return Err(ServerOtelRuntimeError::InvalidExporterEndpoint(
            "embedded credentials/userinfo are forbidden".into(),
        ));
    }
    if uri.query().is_some() {
        return Err(ServerOtelRuntimeError::InvalidExporterEndpoint(
            "query strings are forbidden".into(),
        ));
    }
    Ok(())
}

pub fn server_otel_runtime_from_resolved(
    config: ResolvedOresOtelConfig,
    environment: &OresOtelEnv,
    expected_service_name: &str,
    logger_name: Option<&str>,
    transports: Vec<Arc<dyn Transport>>,
) -> Result<ServerOtelRuntime, ServerOtelRuntimeError> {
    if let Some(actual) = config.service_name.as_deref()
        && actual != expected_service_name
    {
        return Err(ServerOtelRuntimeError::ServiceNameMismatch {
            expected: expected_service_name.to_owned(),
            actual: actual.to_owned(),
        });
    }
    if config.logging.auto_send {
        return Err(ServerOtelRuntimeError::UnsupportedAutoSend);
    }

    let exporter_endpoint = if config.enabled
        && config.tracing.enabled
        && config.exporter.protocol != OresOtelExporterProtocol::None
    {
        let endpoint_env = config
            .exporter
            .endpoint_env
            .as_deref()
            .ok_or(ServerOtelRuntimeError::MissingExporterEndpointEnv)?;
        let endpoint = resolve_exporter_endpoint(&config, environment).ok_or_else(|| {
            ServerOtelRuntimeError::MissingExporterEndpoint(endpoint_env.to_owned())
        })?;
        validate_exporter_endpoint(&endpoint)?;
        if !transports
            .iter()
            .any(|transport| transport.is_open_telemetry())
        {
            return Err(ServerOtelRuntimeError::MissingOpenTelemetryTransport);
        }
        Some(endpoint)
    } else {
        None
    };

    let logger = (config.enabled && config.logging.enabled).then(|| {
        Logger::new(Options {
            app_name: expected_service_name.to_owned(),
            name: logger_name.map(str::to_owned),
            max_level: log_level(config.logging.level),
            console: config.logging.console,
            transports,
            otel_enabled: config.tracing.enabled,
            ..Options::default()
        })
    });

    Ok(ServerOtelRuntime {
        config,
        logger,
        exporter_endpoint,
    })
}

pub fn load_server_otel_runtime(
    expected_service_name: &str,
    logger_name: Option<&str>,
    mut options: LoadOptions,
    transports: Vec<Arc<dyn Transport>>,
) -> Result<ServerOtelRuntime, ServerOtelRuntimeError> {
    options.resolve.role = Some(RuntimeRole::Server);
    let environment = options.resolve.effective_env();
    let loaded = load_ores_otel_config(options)?;
    server_otel_runtime_from_resolved(
        loaded.config,
        &environment,
        expected_service_name,
        logger_name,
        transports,
    )
}

pub fn load_server_otel_runtime_from_process_env(
    expected_service_name: &str,
    logger_name: Option<&str>,
    transports: Vec<Arc<dyn Transport>>,
) -> Result<ServerOtelRuntime, ServerOtelRuntimeError> {
    load_server_otel_runtime(
        expected_service_name,
        logger_name,
        LoadOptions::from_process_env(),
        transports,
    )
}

/// Deterministically decides whether one trace is exported. The hash keeps the
/// decision stable across every event in the same trace without mutable sampler
/// state and works for both generated and externally supplied trace identifiers.
#[must_use]
pub fn should_sample_trace(trace_id: &str, sample_ratio: f64) -> bool {
    if sample_ratio <= 0.0 {
        return false;
    }
    if sample_ratio >= 1.0 {
        return true;
    }
    let digest = Sha256::digest(trace_id.as_bytes());
    let bucket = u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("sha256 prefix is eight bytes"),
    );
    let threshold = (sample_ratio * u64::MAX as f64) as u64;
    bucket <= threshold
}

/// Maps the portable, serializable middleware context into ores-otel's native
/// task context. Only allow-listed correlation metadata is copied.
pub fn to_ores_log_context(context: &RequestContext) -> LogContext {
    let fields = [
        Some((
            "request.id".into(),
            Value::String(context.request_id.clone()),
        )),
        Some(("trace.id".into(), Value::String(context.trace_id.clone()))),
        Some((
            "request.started_at_unix_ms".into(),
            Value::from(context.started_at_unix_ms),
        )),
        context
            .user_id
            .as_ref()
            .map(|user_id| ("user.id".into(), Value::String(user_id.clone()))),
        context
            .tenant_id
            .as_ref()
            .map(|tenant_id| ("tenant.id".into(), Value::String(tenant_id.clone()))),
        context
            .locale
            .as_ref()
            .map(|locale| ("request.locale".into(), Value::String(locale.clone()))),
        context
            .deadline_unix_ms
            .map(|deadline| ("request.deadline_unix_ms".into(), Value::from(deadline))),
    ]
    .into_iter()
    .flatten()
    .collect::<JsonObject>();

    let logged_in_user = context
        .user_id
        .as_ref()
        .map(|user_id| JsonObject::from_iter([("id".into(), Value::String(user_id.clone()))]))
        .unwrap_or_default();
    let baggage = context
        .baggage
        .iter()
        .filter(|(key, _)| key.starts_with("otel."))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    let trace_ids = if context.trace_id.is_empty() {
        Vec::new()
    } else {
        vec![context.trace_id.clone()]
    };

    LogContext {
        logged_in_user,
        fields,
        trace_id: (!context.trace_id.is_empty()).then(|| context.trace_id.clone()),
        trace_ids,
        span_id: context.span_id.clone(),
        baggage,
        routine_id: Some(context.request_id.clone()),
        tags: vec!["ores-middleware".into(), "request".into()],
        ..LogContext::default()
    }
    .normalized()
}

/// A request-specific logger handle suitable for Axum request extensions.
/// Unlike cloning the root logger alone, every event is permanently decorated
/// with the immutable request/user/tenant snapshot captured at construction.
#[derive(Clone)]
pub struct RequestLogger {
    logger: Logger,
    context: LogContext,
    otel_sampled: bool,
}

impl RequestLogger {
    pub fn new(logger: Logger, context: &RequestContext) -> Self {
        Self::new_with_sampling(logger, context, true, 1.0)
    }

    pub fn new_with_sampling(
        logger: Logger,
        context: &RequestContext,
        tracing_enabled: bool,
        sample_ratio: f64,
    ) -> Self {
        Self {
            logger,
            context: to_ores_log_context(context),
            otel_sampled: tracing_enabled && should_sample_trace(&context.trace_id, sample_ratio),
        }
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    pub fn context(&self) -> &LogContext {
        &self.context
    }

    pub fn otel_sampled(&self) -> bool {
        self.otel_sampled
    }

    pub fn trace(&self, values: Vec<Value>) -> Event {
        apply_log_context(self.logger.trace(values), &self.context).with_otel(self.otel_sampled)
    }

    pub fn debug(&self, values: Vec<Value>) -> Event {
        apply_log_context(self.logger.debug(values), &self.context).with_otel(self.otel_sampled)
    }

    pub fn info(&self, values: Vec<Value>) -> Event {
        apply_log_context(self.logger.info(values), &self.context).with_otel(self.otel_sampled)
    }

    pub fn warn(&self, values: Vec<Value>) -> Event {
        apply_log_context(self.logger.warn(values), &self.context).with_otel(self.otel_sampled)
    }

    pub fn error(&self, values: Vec<Value>) -> Event {
        apply_log_context(self.logger.error(values), &self.context).with_otel(self.otel_sampled)
    }

    pub fn fatal(&self, values: Vec<Value>) -> Event {
        apply_log_context(self.logger.fatal(values), &self.context).with_otel(self.otel_sampled)
    }
}

/// Runs a future with ores-otel's poll-safe task context. File/module loggers
/// imported elsewhere can call `info_context`, `warn_context`, and friends and
/// still receive this request's correlation fields.
pub fn run_with_ores_log_context<F: Future>(
    context: &RequestContext,
    future: F,
) -> ContextFuture<F> {
    with_log_context_async(to_ores_log_context(context), future)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use super::*;

    fn request_context() -> RequestContext {
        RequestContext {
            request_id: "request-42".into(),
            trace_id: "0123456789abcdef0123456789abcdef".into(),
            span_id: Some("0123456789abcdef".into()),
            tenant_id: Some("tenant-7".into()),
            user_id: Some("user-42".into()),
            locale: Some("en-US".into()),
            started_at_unix_ms: 1,
            deadline_unix_ms: Some(2),
            baggage: BTreeMap::from([
                ("otel.vendor".into(), "allowed".into()),
                ("authorization".into(), "must-not-propagate".into()),
            ]),
        }
    }

    fn resolved(input: &str) -> ResolvedOresOtelConfig {
        let parsed = parse_ores_otel_toml(input).expect("valid telemetry fixture");
        resolve_ores_otel_config(
            &parsed,
            &ResolveOptions::default().with_role(RuntimeRole::Server),
        )
        .expect("resolved server telemetry")
    }

    #[test]
    fn mapped_log_context_owns_fresh_metadata_collections() {
        let source = request_context();
        let mut mapped = to_ores_log_context(&source);

        mapped
            .fields
            .insert("tenant.id".into(), Value::String("changed".into()));
        mapped
            .baggage
            .insert("otel.vendor".into(), "changed".into());

        assert_eq!(source.tenant_id.as_deref(), Some("tenant-7"));
        assert_eq!(
            source.baggage.get("otel.vendor").map(String::as_str),
            Some("allowed")
        );
    }

    #[test]
    fn resolved_logging_controls_level_and_disablement() {
        let config = resolved(
            r#"
version = 1
[server]
service_name = "svc"
[server.logging]
enabled = true
level = "error"
console = false
"#,
        );
        let transport = Arc::new(MemoryTransport::default());
        let runtime = server_otel_runtime_from_resolved(
            config,
            &OresOtelEnv::new(),
            "svc",
            Some("http"),
            vec![transport.clone()],
        )
        .expect("runtime");
        let logger = runtime.logger.expect("logger enabled");
        assert!(
            logger
                .info(vec![Value::String("filtered".into())])
                .send()
                .unwrap()
                .is_none()
        );
        assert!(
            logger
                .error(vec![Value::String("kept".into())])
                .send()
                .unwrap()
                .is_some()
        );
        assert_eq!(transport.records().len(), 1);

        let disabled = resolved(
            r#"
version = 1
[server]
service_name = "svc"
[server.logging]
enabled = false
"#,
        );
        let runtime = server_otel_runtime_from_resolved(
            disabled,
            &OresOtelEnv::new(),
            "svc",
            None,
            Vec::new(),
        )
        .expect("disabled runtime");
        assert!(runtime.logger.is_none());
    }

    #[test]
    fn unsupported_auto_send_and_service_drift_fail_closed() {
        let auto_send = resolved(
            r#"
version = 1
[server]
service_name = "svc"
[server.logging]
auto_send = true
"#,
        );
        assert!(matches!(
            server_otel_runtime_from_resolved(
                auto_send,
                &OresOtelEnv::new(),
                "svc",
                None,
                Vec::new()
            ),
            Err(ServerOtelRuntimeError::UnsupportedAutoSend)
        ));

        let drift = resolved("version = 1\n[server]\nservice_name = \"other\"\n");
        assert!(matches!(
            server_otel_runtime_from_resolved(drift, &OresOtelEnv::new(), "svc", None, Vec::new()),
            Err(ServerOtelRuntimeError::ServiceNameMismatch { .. })
        ));
    }

    #[test]
    fn exporter_requires_valid_endpoint_and_explicit_otel_transport() {
        let config = resolved(
            r#"
version = 1
[server]
service_name = "svc"
[server.exporter]
protocol = "otlp_http"
endpoint_env = "OTEL_EXPORTER_OTLP_ENDPOINT"
"#,
        );
        let missing = server_otel_runtime_from_resolved(
            config.clone(),
            &OresOtelEnv::new(),
            "svc",
            None,
            Vec::new(),
        );
        assert!(matches!(
            missing,
            Err(ServerOtelRuntimeError::MissingExporterEndpoint(_))
        ));

        let credential_env = OresOtelEnv::from([(
            "OTEL_EXPORTER_OTLP_ENDPOINT".into(),
            "https://user:secret@otel.example.com/v1/logs".into(),
        )]);
        assert!(matches!(
            server_otel_runtime_from_resolved(
                config.clone(),
                &credential_env,
                "svc",
                None,
                Vec::new(),
            ),
            Err(ServerOtelRuntimeError::InvalidExporterEndpoint(_))
        ));

        let valid_env = OresOtelEnv::from([(
            "OTEL_EXPORTER_OTLP_ENDPOINT".into(),
            "https://otel.example.com/v1/logs".into(),
        )]);
        assert!(matches!(
            server_otel_runtime_from_resolved(config, &valid_env, "svc", None, Vec::new()),
            Err(ServerOtelRuntimeError::MissingOpenTelemetryTransport)
        ));
    }

    #[test]
    fn sampling_is_stable_and_honors_extremes() {
        let trace = "0123456789abcdef0123456789abcdef";
        assert!(!should_sample_trace(trace, 0.0));
        assert!(should_sample_trace(trace, 1.0));
        assert_eq!(
            should_sample_trace(trace, 0.5),
            should_sample_trace(trace, 0.5)
        );
        let context = request_context();
        let logger = Logger::new(Options {
            console: false,
            ..Options::default()
        });
        assert!(
            !RequestLogger::new_with_sampling(logger.clone(), &context, false, 1.0).otel_sampled()
        );
        assert!(
            !RequestLogger::new_with_sampling(logger.clone(), &context, true, 0.0).otel_sampled()
        );
        assert!(RequestLogger::new_with_sampling(logger, &context, true, 1.0).otel_sampled());
    }

    #[tokio::test]
    async fn request_and_file_loggers_share_poll_safe_context() {
        let transport = Arc::new(MemoryTransport::default());
        let logger = Logger::new(Options {
            app_name: "middleware-test".into(),
            name: Some("orders".into()),
            transports: vec![transport.clone()],
            console: false,
            ..Options::default()
        });
        let context = request_context();
        let request_logger = RequestLogger::new(logger.clone(), &context);

        request_logger
            .warn(vec![Value::String("slow dependency".into())])
            .send()
            .expect("request log");
        run_with_ores_log_context(&context, async {
            logger
                .info_context(vec![Value::String("handler reached".into())])
                .send()
                .expect("file log");
        })
        .await;

        let records = transport.records();
        assert_eq!(records.len(), 2);
        for record in records {
            assert_eq!(record.fields["request.id"], "request-42");
            assert_eq!(record.fields["tenant.id"], "tenant-7");
            assert_eq!(
                record
                    .logged_in_user
                    .as_ref()
                    .and_then(|user| user.get("id")),
                Some(&Value::String("user-42".into()))
            );
            let baggage = record.fields["otel.baggage"]
                .as_object()
                .expect("otel baggage object");
            assert_eq!(
                baggage.get("otel.vendor"),
                Some(&Value::String("allowed".into()))
            );
            assert!(!baggage.contains_key("authorization"));
        }
        assert_eq!(current_log_context(), LogContext::default());
    }
}

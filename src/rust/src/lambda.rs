use std::{
    collections::BTreeMap,
    env, fmt,
    fs::{self, File},
    future::Future,
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

use uuid::Uuid;

use crate::{
    BootstrapError, ManifestLoadError, OperationDescriptor, OperationOutcome, OperationScope,
    OperationTransport, RequestContext, admit_server_stack_from_env, default_config,
    run_operation_boundary_with_timeout, run_operation_boundary_with_timeout_and_cancellation,
};

const MAX_TOKEN_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LambdaInvocationTrigger {
    Queue,
    Schedule,
    Direct,
}

impl LambdaInvocationTrigger {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Schedule => "schedule",
            Self::Direct => "direct",
        }
    }
}

/// Trusted adapter metadata for a non-HTTP Lambda callback.
///
/// Provider names and public payload fields are deliberately absent. The adapter
/// supplies only bounded invocation metadata that it obtained from its runtime
/// API. In particular, payload data cannot establish user or tenant identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LambdaInvocationMetadata {
    pub invocation_id: String,
    pub trace_id: Option<String>,
    pub function_name: String,
    pub trigger: LambdaInvocationTrigger,
    pub payload_bytes: u64,
    pub deadline_unix_ms: Option<u64>,
}

#[derive(Debug)]
pub enum LambdaInvocationError {
    Manifest(ManifestLoadError),
    Bootstrap(BootstrapError),
    InvalidMetadata(&'static str),
    InvalidStackConfig { path: PathBuf },
    PayloadTooLarge { payload_bytes: u64, limit: u64 },
}

impl LambdaInvocationError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Manifest(error) => error.code(),
            Self::Bootstrap(error) => error.code,
            Self::InvalidMetadata(_) => "invalid_lambda_invocation_metadata",
            Self::InvalidStackConfig { .. } => "lambda_stack_config_invalid",
            Self::PayloadTooLarge { .. } => "lambda_payload_too_large",
        }
    }
}

impl fmt::Display for LambdaInvocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(error) => {
                write!(formatter, "lambda middleware manifest admission failed: {error}")
            }
            Self::Bootstrap(error) => {
                write!(formatter, "lambda middleware bootstrap failed: {error}")
            }
            Self::InvalidMetadata(field) => {
                write!(formatter, "invalid trusted Lambda metadata field: {field}")
            }
            Self::InvalidStackConfig { path } => write!(
                formatter,
                "Lambda middleware stack config is not a readable regular file: {}",
                path.display()
            ),
            Self::PayloadTooLarge {
                payload_bytes,
                limit,
            } => write!(
                formatter,
                "Lambda payload exceeds configured middleware limit ({payload_bytes} > {limit})"
            ),
        }
    }
}

impl std::error::Error for LambdaInvocationError {}

/// The callback-native subset currently enforced before user code runs.
///
/// HTTP-only policy (TLS/forwarded headers, response security headers,
/// compression) and callback auth/rate/idempotency are intentionally absent
/// until they have trusted callback-native inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LambdaInvocationPolicy {
    timeout_ms: u64,
    max_payload_bytes: u64,
}

/// Fail-closed, provider-neutral middleware boundary for queue, schedule and
/// direct Lambda callbacks.
///
/// HTTP-triggered functions should continue to use the framework/Axum adapter so
/// TLS, forwarded-header, CORS and response-header policy stay on the HTTP path.
/// This boundary admits `.ores-mw.toml`, requires its selected stack policy file
/// to be present and readable, resolves the callback-native payload/deadline
/// subset from the canonical environment names, and executes the callback
/// through the first-class `lambda` operation boundary with task-local
/// request/log context. It intentionally does not fabricate HTTP metadata or
/// require HTTP-only TLS settings in production.
#[derive(Clone)]
pub struct LambdaInvocationBoundary {
    policy: LambdaInvocationPolicy,
    manifest_path: PathBuf,
    target_name: Option<String>,
    stack_config: String,
}

impl LambdaInvocationBoundary {
    /// Discover and admit `.ores-mw.toml`, prove the selected server stack path
    /// names a readable regular non-symlink file, and resolve the callback-native
    /// middleware limits from the same canonical environment names used by the
    /// server bootstrap.
    pub fn from_env(
        service_name: impl Into<String>,
        target_name: Option<&str>,
        expected_stack_config: &str,
    ) -> Result<Self, LambdaInvocationError> {
        let manifest_path = admit_server_stack_from_env(target_name, expected_stack_config)
            .map_err(LambdaInvocationError::Manifest)?;
        let stack_config_path = admit_stack_config_file(expected_stack_config)?;
        let policy = invocation_policy_from_lookup(service_name.into(), |name| {
            env::var(name).ok()
        })
        .map_err(LambdaInvocationError::Bootstrap)?;
        Ok(Self {
            policy,
            manifest_path,
            target_name: target_name.map(ToOwned::to_owned),
            stack_config: stack_config_path.to_string_lossy().into_owned(),
        })
    }

    #[must_use]
    pub fn manifest_path(&self) -> &std::path::Path {
        &self.manifest_path
    }

    #[must_use]
    pub fn target_name(&self) -> Option<&str> {
        self.target_name.as_deref()
    }

    #[must_use]
    pub fn stack_config(&self) -> &str {
        &self.stack_config
    }

    /// Execute one non-HTTP Lambda callback with the middleware timeout and any
    /// earlier provider runtime deadline. Invalid/expired budgets fail before the
    /// callback future is polled.
    pub async fn run<F, T, E>(
        &self,
        metadata: LambdaInvocationMetadata,
        operation: F,
    ) -> Result<OperationOutcome<T>, LambdaInvocationError>
    where
        F: Future<Output = Result<T, E>>,
    {
        let admitted = self.admit(metadata)?;
        Ok(run_operation_boundary_with_timeout(
            admitted.context,
            admitted.descriptor,
            admitted.timeout,
            operation,
        )
        .await)
    }

    /// Cancellation-aware form for runtimes that expose a shutdown/abort future.
    /// Cancellation wins a simultaneous readiness tie, matching the shared
    /// operation boundary semantics.
    pub async fn run_with_cancellation<F, C, T, E>(
        &self,
        metadata: LambdaInvocationMetadata,
        cancellation: C,
        operation: F,
    ) -> Result<OperationOutcome<T>, LambdaInvocationError>
    where
        F: Future<Output = Result<T, E>>,
        C: Future<Output = ()>,
    {
        let admitted = self.admit(metadata)?;
        Ok(run_operation_boundary_with_timeout_and_cancellation(
            admitted.context,
            admitted.descriptor,
            admitted.timeout,
            cancellation,
            operation,
        )
        .await)
    }

    fn admit(
        &self,
        metadata: LambdaInvocationMetadata,
    ) -> Result<AdmittedInvocation, LambdaInvocationError> {
        if !valid_token(&metadata.invocation_id) {
            return Err(LambdaInvocationError::InvalidMetadata("invocation_id"));
        }
        if !valid_token(&metadata.function_name) {
            return Err(LambdaInvocationError::InvalidMetadata("function_name"));
        }
        if metadata.payload_bytes > self.policy.max_payload_bytes {
            return Err(LambdaInvocationError::PayloadTooLarge {
                payload_bytes: metadata.payload_bytes,
                limit: self.policy.max_payload_bytes,
            });
        }

        let trace_id = match metadata.trace_id {
            Some(trace_id) if valid_trace_id(&trace_id) => trace_id.to_ascii_lowercase(),
            Some(_) => return Err(LambdaInvocationError::InvalidMetadata("trace_id")),
            None => Uuid::new_v4().simple().to_string(),
        };
        let now = RequestContext::now_ms();
        let configured_deadline = now.saturating_add(self.policy.timeout_ms);
        let deadline_unix_ms = metadata
            .deadline_unix_ms
            .map_or(configured_deadline, |provider| provider.min(configured_deadline));
        let timeout = Duration::from_millis(deadline_unix_ms.saturating_sub(now));

        let baggage = BTreeMap::from([
            (
                "otel.lambda.trigger".to_owned(),
                metadata.trigger.as_str().to_owned(),
            ),
            (
                "otel.lambda.function".to_owned(),
                metadata.function_name.clone(),
            ),
        ]);
        let context = RequestContext {
            request_id: metadata.invocation_id,
            trace_id,
            span_id: None,
            tenant_id: None,
            user_id: None,
            locale: None,
            started_at_unix_ms: now,
            deadline_unix_ms: Some(deadline_unix_ms),
            baggage,
        };
        let descriptor = OperationDescriptor {
            transport: OperationTransport::Lambda,
            scope: OperationScope::Callback,
            name: format!("lambda.{}", metadata.function_name),
        };
        Ok(AdmittedInvocation {
            context,
            descriptor,
            timeout,
        })
    }

    #[cfg(test)]
    fn from_policy_for_test(policy: LambdaInvocationPolicy) -> Self {
        Self {
            policy,
            manifest_path: PathBuf::from(".ores-mw.toml"),
            target_name: Some("lambda".into()),
            stack_config: "config/middleware.json".into(),
        }
    }
}

struct AdmittedInvocation {
    context: RequestContext,
    descriptor: OperationDescriptor,
    timeout: Duration,
}

fn invocation_policy_from_lookup<F>(
    service_name: String,
    lookup: F,
) -> Result<LambdaInvocationPolicy, BootstrapError>
where
    F: Fn(&str) -> Option<String>,
{
    let defaults = default_config(service_name);
    if let Some(value) = first_value(&lookup, &["ORES_MIDDLEWARE_ENV", "APP_ENV", "RUST_ENV"]) {
        validate_environment(&value)?;
    }
    let timeout_ms = parse_positive(
        lookup("ORES_MIDDLEWARE_TIMEOUT_MS"),
        "ORES_MIDDLEWARE_TIMEOUT_MS",
        defaults.settings.timeout_ms,
    )?;
    let max_payload_bytes = parse_positive(
        lookup("ORES_MIDDLEWARE_MAX_BODY_BYTES"),
        "ORES_MIDDLEWARE_MAX_BODY_BYTES",
        defaults.settings.max_body_bytes as u64,
    )?;
    Ok(LambdaInvocationPolicy {
        timeout_ms,
        max_payload_bytes,
    })
}

fn first_value<F>(lookup: &F, names: &[&str]) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    names.iter().find_map(|name| lookup(name))
}

fn validate_environment(value: &str) -> Result<(), BootstrapError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "development" | "dev" | "local" | "test" | "testing" | "staging" | "stage"
        | "production" | "prod" => Ok(()),
        other => Err(BootstrapError {
            variable: Some("ORES_MIDDLEWARE_ENV".into()),
            code: "invalid_value",
            message: format!("unsupported runtime environment {other:?}"),
        }),
    }
}

fn parse_positive<T>(
    value: Option<String>,
    variable: &'static str,
    default: T,
) -> Result<T, BootstrapError>
where
    T: FromStr + PartialEq + Default,
{
    let Some(value) = value else {
        return Ok(default);
    };
    let parsed = value.trim().parse::<T>().map_err(|_| BootstrapError {
        variable: Some(variable.into()),
        code: "invalid_number",
        message: format!("expected a positive numeric value, got {value:?}"),
    })?;
    if parsed == T::default() {
        return Err(BootstrapError {
            variable: Some(variable.into()),
            code: "invalid_number",
            message: "value must be positive".into(),
        });
    }
    Ok(parsed)
}

fn admit_stack_config_file(path: &str) -> Result<PathBuf, LambdaInvocationError> {
    let path = PathBuf::from(path);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| LambdaInvocationError::InvalidStackConfig { path: path.clone() })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(LambdaInvocationError::InvalidStackConfig { path });
    }
    File::open(&path)
        .map_err(|_| LambdaInvocationError::InvalidStackConfig { path: path.clone() })?;
    Ok(path)
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
        })
}

fn valid_trace_id(value: &str) -> bool {
    value.len() == 32
        && value != "00000000000000000000000000000000"
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    use super::*;

    fn boundary() -> LambdaInvocationBoundary {
        let defaults = default_config("lambda-test");
        LambdaInvocationBoundary::from_policy_for_test(LambdaInvocationPolicy {
            timeout_ms: defaults.settings.timeout_ms,
            max_payload_bytes: defaults.settings.max_body_bytes as u64,
        })
    }

    fn metadata() -> LambdaInvocationMetadata {
        LambdaInvocationMetadata {
            invocation_id: "invoke-1".into(),
            trace_id: Some("0123456789abcdef0123456789abcdef".into()),
            function_name: "orders-consume".into(),
            trigger: LambdaInvocationTrigger::Queue,
            payload_bytes: 32,
            deadline_unix_ms: None,
        }
    }

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ores-mw-lambda-stack-{tag}-{}", Uuid::new_v4()))
    }

    fn policy_from(values: &[(&str, &str)]) -> Result<LambdaInvocationPolicy, BootstrapError> {
        let values = values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<BTreeMap<_, _>>();
        invocation_policy_from_lookup("lambda-test".into(), |name| values.get(name).cloned())
    }

    #[test]
    fn production_callback_policy_does_not_require_http_tls_mode() {
        let policy = policy_from(&[("ORES_MIDDLEWARE_ENV", "production")]).unwrap();
        assert!(policy.timeout_ms > 0);
        assert!(policy.max_payload_bytes > 0);
    }

    #[test]
    fn callback_policy_uses_shared_limit_environment_names() {
        let policy = policy_from(&[
            ("ORES_MIDDLEWARE_ENV", "prod"),
            ("ORES_MIDDLEWARE_TIMEOUT_MS", "2750"),
            ("ORES_MIDDLEWARE_MAX_BODY_BYTES", "8192"),
        ])
        .unwrap();
        assert_eq!(policy.timeout_ms, 2_750);
        assert_eq!(policy.max_payload_bytes, 8_192);
    }

    #[test]
    fn callback_policy_rejects_invalid_environment_and_zero_limits() {
        assert_eq!(
            policy_from(&[("ORES_MIDDLEWARE_ENV", "mystery")])
                .unwrap_err()
                .code,
            "invalid_value"
        );
        assert_eq!(
            policy_from(&[("ORES_MIDDLEWARE_TIMEOUT_MS", "0")])
                .unwrap_err()
                .code,
            "invalid_number"
        );
        assert_eq!(
            policy_from(&[("ORES_MIDDLEWARE_MAX_BODY_BYTES", "0")])
                .unwrap_err()
                .code,
            "invalid_number"
        );
    }

    #[test]
    fn stack_config_admission_accepts_a_readable_regular_file() {
        let root = temp_path("regular");
        fs::create_dir_all(&root).expect("fixture root");
        let path = root.join("middleware.json");
        fs::write(&path, b"{}\n").expect("fixture stack");

        assert_eq!(
            admit_stack_config_file(path.to_str().expect("utf8 path")).unwrap(),
            path
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn stack_config_admission_rejects_missing_and_directory_paths() {
        let root = temp_path("invalid");
        fs::create_dir_all(&root).expect("fixture root");
        let missing = root.join("missing.json");
        assert!(matches!(
            admit_stack_config_file(missing.to_str().expect("utf8 path")),
            Err(LambdaInvocationError::InvalidStackConfig { .. })
        ));
        assert!(matches!(
            admit_stack_config_file(root.to_str().expect("utf8 path")),
            Err(LambdaInvocationError::InvalidStackConfig { .. })
        ));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn stack_config_admission_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let root = temp_path("symlink");
        fs::create_dir_all(&root).expect("fixture root");
        let target = root.join("target.json");
        let link = root.join("stack.json");
        fs::write(&target, b"{}\n").expect("fixture target");
        symlink(&target, &link).expect("fixture symlink");

        assert!(matches!(
            admit_stack_config_file(link.to_str().expect("utf8 path")),
            Err(LambdaInvocationError::InvalidStackConfig { .. })
        ));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn callback_runs_with_lambda_context_and_no_payload_identity() {
        let outcome = boundary()
            .run(metadata(), async {
                let context = crate::current_context().expect("callback context");
                assert_eq!(context.request_id, "invoke-1");
                assert_eq!(context.user_id, None);
                assert_eq!(context.tenant_id, None);
                assert_eq!(
                    context.baggage.get("otel.lambda.trigger").map(String::as_str),
                    Some("queue")
                );
                Ok::<_, Infallible>(42)
            })
            .await
            .expect("metadata admitted");
        assert_eq!(outcome, OperationOutcome::Completed(42));
        assert!(crate::current_context().is_none());
    }

    #[tokio::test]
    async fn oversized_payload_is_rejected_before_callback_poll() {
        let mut input = metadata();
        input.payload_bytes = u64::MAX;
        let polled = Arc::new(AtomicBool::new(false));
        let called = polled.clone();
        let result = boundary()
            .run(input, async move {
                called.store(true, Ordering::SeqCst);
                Ok::<_, Infallible>(())
            })
            .await;
        assert!(matches!(
            result,
            Err(LambdaInvocationError::PayloadTooLarge { .. })
        ));
        assert!(!polled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn expired_provider_deadline_never_polls_callback() {
        let mut input = metadata();
        input.deadline_unix_ms = Some(0);
        let polled = Arc::new(AtomicBool::new(false));
        let called = polled.clone();
        let outcome = boundary()
            .run(input, async move {
                called.store(true, Ordering::SeqCst);
                Ok::<_, Infallible>(())
            })
            .await
            .expect("metadata admitted");
        assert!(matches!(
            outcome.failure().map(|failure| failure.kind),
            Some(crate::OperationFailureKind::DeadlineExceeded)
        ));
        assert!(!polled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn invalid_trace_id_is_rejected_before_callback_poll() {
        let mut input = metadata();
        input.trace_id = Some("caller-controlled-not-a-trace".into());
        let polled = Arc::new(AtomicBool::new(false));
        let called = polled.clone();
        let result = boundary()
            .run(input, async move {
                called.store(true, Ordering::SeqCst);
                Ok::<_, Infallible>(())
            })
            .await;
        assert!(matches!(
            result,
            Err(LambdaInvocationError::InvalidMetadata("trace_id"))
        ));
        assert!(!polled.load(Ordering::SeqCst));
    }
}

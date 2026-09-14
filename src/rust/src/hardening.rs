use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::Value;
use tokio::{
    sync::Mutex,
    task::JoinError,
    time::{Instant, timeout_at},
};

use crate::{
    resilience::{CircuitBreaker, CircuitBreakerConfig, ResilienceConfigError},
    stage::{MiddlewareStageHandler, StageDecision, StageInput, StageResponse},
};

pub const MAX_RAW_HEADERS: usize = 128;
pub const MAX_HEADER_NAME_BYTES: usize = 128;
pub const MAX_HEADER_VALUE_BYTES: usize = 8 * 1024;
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_STAGE_ATTRIBUTES: usize = 32;
pub const MAX_STAGE_ATTRIBUTE_KEY_BYTES: usize = 64;
pub const MAX_STAGE_ATTRIBUTE_VALUE_BYTES: usize = 1024;
pub const MAX_STAGE_ATTRIBUTE_BYTES: usize = 8 * 1024;
pub const MAX_HARDENED_STAGES: usize = 64;
pub const MAX_BREAKER_REGISTRY_ENTRIES: usize = 1024;
pub const MAX_BREAKER_KEY_COMPONENT_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderAdmissionError {
    pub code: &'static str,
}

impl HeaderAdmissionError {
    const fn new(code: &'static str) -> Self {
        Self { code }
    }
}

/// Admit raw header pairs before any framework adapter projects them into a map.
/// Security-sensitive duplicates are rejected rather than collapsed. Only a small,
/// explicit RFC-list allowlist is joined.
pub fn admit_raw_headers(
    raw_headers: &[(String, String)],
) -> Result<BTreeMap<String, String>, HeaderAdmissionError> {
    if raw_headers.len() > MAX_RAW_HEADERS {
        return Err(HeaderAdmissionError::new("header_count_exceeded"));
    }

    let mut total_bytes = 0usize;
    let mut admitted = BTreeMap::<String, String>::new();

    for (raw_name, raw_value) in raw_headers {
        if raw_name.is_empty() || raw_name.len() > MAX_HEADER_NAME_BYTES {
            return Err(HeaderAdmissionError::new("header_name_size_invalid"));
        }
        if raw_value.len() > MAX_HEADER_VALUE_BYTES {
            return Err(HeaderAdmissionError::new("header_value_size_exceeded"));
        }
        if !raw_name.bytes().all(is_http_token_byte) {
            return Err(HeaderAdmissionError::new("header_name_invalid"));
        }
        if raw_value.bytes().any(is_forbidden_header_value_byte) {
            return Err(HeaderAdmissionError::new("header_value_invalid"));
        }

        total_bytes = total_bytes
            .checked_add(raw_name.len())
            .and_then(|value| value.checked_add(raw_value.len()))
            .ok_or_else(|| HeaderAdmissionError::new("header_bytes_exceeded"))?;
        if total_bytes > MAX_HEADER_BYTES {
            return Err(HeaderAdmissionError::new("header_bytes_exceeded"));
        }

        let name = raw_name.to_ascii_lowercase();
        if admitted.contains_key(&name) {
            if is_singleton_or_security_sensitive(&name) {
                return Err(HeaderAdmissionError::new("duplicate_header_forbidden"));
            }
            if !is_joinable_list_header(&name) {
                return Err(HeaderAdmissionError::new("duplicate_header_ambiguous"));
            }
            let existing = admitted.get_mut(&name).expect("known header");
            if !existing.is_empty() && !raw_value.is_empty() {
                existing.push_str(", ");
            }
            existing.push_str(raw_value);
        } else {
            admitted.insert(name, raw_value.clone());
        }
    }

    if admitted.contains_key("content-length") && admitted.contains_key("transfer-encoding") {
        return Err(HeaderAdmissionError::new("transfer_length_ambiguous"));
    }

    Ok(admitted)
}

const fn is_http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

const fn is_forbidden_header_value_byte(byte: u8) -> bool {
    byte == 0 || byte == b'\r' || byte == b'\n' || (byte < 0x20 && byte != b'\t') || byte == 0x7f
}

fn is_singleton_or_security_sensitive(name: &str) -> bool {
    matches!(
        name,
        "content-length"
            | "transfer-encoding"
            | "authorization"
            | "cookie"
            | "host"
            | "origin"
            | "forwarded"
            | "cf-connecting-ip"
            | "traceparent"
            | "idempotency-key"
    ) || name.starts_with("x-forwarded-")
        || name.starts_with("x-ores-")
}

fn is_joinable_list_header(name: &str) -> bool {
    matches!(
        name,
        "accept"
            | "accept-encoding"
            | "accept-language"
            | "cache-control"
            | "pragma"
            | "via"
            | "warning"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageAttributeError {
    pub code: &'static str,
}

pub fn validate_stage_attributes(
    attributes: &BTreeMap<String, Value>,
) -> Result<(), StageAttributeError> {
    if attributes.len() > MAX_STAGE_ATTRIBUTES {
        return Err(StageAttributeError {
            code: "stage_attribute_count_exceeded",
        });
    }

    let mut total_bytes = 0usize;
    for (key, value) in attributes {
        if key.is_empty()
            || key.len() > MAX_STAGE_ATTRIBUTE_KEY_BYTES
            || !key.bytes().all(is_stage_attribute_key_byte)
        {
            return Err(StageAttributeError {
                code: "stage_attribute_key_invalid",
            });
        }
        let encoded = serde_json::to_vec(value).map_err(|_| StageAttributeError {
            code: "stage_attribute_value_invalid",
        })?;
        if encoded.len() > MAX_STAGE_ATTRIBUTE_VALUE_BYTES {
            return Err(StageAttributeError {
                code: "stage_attribute_value_exceeded",
            });
        }
        total_bytes = total_bytes
            .checked_add(key.len())
            .and_then(|size| size.checked_add(encoded.len()))
            .ok_or(StageAttributeError {
                code: "stage_attribute_bytes_exceeded",
            })?;
        if total_bytes > MAX_STAGE_ATTRIBUTE_BYTES {
            return Err(StageAttributeError {
                code: "stage_attribute_bytes_exceeded",
            });
        }
    }
    Ok(())
}

const fn is_stage_attribute_key_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDocument {
    #[serde(rename = "type")]
    pub problem_type: &'static str,
    pub title: &'static str,
    pub status: u16,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

pub fn sanitized_problem_response(
    status: u16,
    code: &str,
    request_id: Option<&str>,
    safe_headers: &BTreeMap<String, String>,
) -> StageResponse {
    let code = sanitize_problem_code(code);
    let request_id = request_id.and_then(sanitize_request_id);
    let body = serde_json::to_vec(&ProblemDocument {
        problem_type: "about:blank",
        title: "Request rejected",
        status,
        code,
        request_id,
    })
    .unwrap_or_else(|_| b"{\"title\":\"Request rejected\"}".to_vec());

    let headers = safe_headers
        .iter()
        .filter_map(|(name, value)| sanitize_safe_response_header(name, value))
        .chain([
            (
                "content-type".to_owned(),
                "application/problem+json".to_owned(),
            ),
            ("cache-control".to_owned(), "no-store".to_owned()),
        ])
        .collect();

    StageResponse {
        status,
        headers,
        body,
    }
}

fn sanitize_problem_code(value: &str) -> String {
    let filtered = value
        .bytes()
        .take(64)
        .filter(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        .map(char::from)
        .collect::<String>();
    if filtered.is_empty() {
        "middleware_rejection".to_owned()
    } else {
        filtered
    }
}

fn sanitize_request_id(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > 128 {
        return None;
    }
    value
        .bytes()
        .all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
        .then(|| value.to_owned())
}

fn sanitize_safe_response_header(name: &str, value: &str) -> Option<(String, String)> {
    let name = name.to_ascii_lowercase();
    if !matches!(
        name.as_str(),
        "retry-after"
            | "ratelimit-limit"
            | "ratelimit-remaining"
            | "ratelimit-reset"
            | "x-request-id"
            | "x-ores-request-id"
    ) || value.is_empty()
        || value.len() > 256
        || value.bytes().any(is_forbidden_header_value_byte)
    {
        return None;
    }
    Some((name, value.to_owned()))
}

#[derive(Debug, Clone)]
pub struct FetchMetadataPolicy {
    pub enabled: bool,
    pub allow_missing: bool,
    pub allow_same_site: bool,
    pub unsafe_methods: BTreeSet<String>,
}

impl Default for FetchMetadataPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_missing: true,
            allow_same_site: true,
            unsafe_methods: ["POST", "PUT", "PATCH", "DELETE"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        }
    }
}

pub struct FetchMetadataStage {
    policy: FetchMetadataPolicy,
}

impl FetchMetadataStage {
    pub fn new(policy: FetchMetadataPolicy) -> Self {
        Self { policy }
    }

    fn unsafe_method(&self, method: &str) -> bool {
        self.policy
            .unsafe_methods
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(method))
    }
}

impl MiddlewareStageHandler for FetchMetadataStage {
    fn name(&self) -> &'static str {
        "fetch-metadata"
    }

    fn request<'a>(
        &'a self,
        input: StageInput,
    ) -> std::pin::Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
        Box::pin(async move {
            if !self.policy.enabled {
                return StageDecision::Continue(Box::new(input));
            }

            for (name, closed_values) in [
                (
                    "sec-fetch-site",
                    Some(&["same-origin", "same-site", "cross-site", "none"][..]),
                ),
                (
                    "sec-fetch-mode",
                    Some(&["cors", "navigate", "no-cors", "same-origin", "websocket"][..]),
                ),
            ] {
                if let Some(value) = header(&input, name)
                    && (!valid_fetch_token(value)
                        || closed_values
                            .is_some_and(|values| !values.iter().any(|item| *item == value)))
                {
                    return StageDecision::Reject(crate::stage::StageRejection::new(
                        403,
                        "fetch_metadata_invalid",
                        "Fetch Metadata policy rejected the request",
                    ));
                }
            }

            if let Some(value) = header(&input, "sec-fetch-dest")
                && !valid_fetch_token(value)
            {
                return StageDecision::Reject(crate::stage::StageRejection::new(
                    403,
                    "fetch_metadata_invalid",
                    "Fetch Metadata policy rejected the request",
                ));
            }
            if let Some(value) = header(&input, "sec-fetch-user")
                && value != "?1"
            {
                return StageDecision::Reject(crate::stage::StageRejection::new(
                    403,
                    "fetch_metadata_invalid",
                    "Fetch Metadata policy rejected the request",
                ));
            }

            let unsafe_method = self.unsafe_method(&input.request.method);
            let ambient_credentials = header(&input, "cookie").is_some();
            if !unsafe_method || !ambient_credentials {
                return StageDecision::Continue(Box::new(input));
            }

            match header(&input, "sec-fetch-site") {
                None if self.policy.allow_missing => StageDecision::Continue(Box::new(input)),
                None => StageDecision::Reject(crate::stage::StageRejection::new(
                    403,
                    "fetch_metadata_required",
                    "Fetch Metadata is required for this request",
                )),
                Some("cross-site") => StageDecision::Reject(crate::stage::StageRejection::new(
                    403,
                    "fetch_metadata_cross_site_denied",
                    "Cross-site ambient-credential mutation denied",
                )),
                Some("same-site") if !self.policy.allow_same_site => {
                    StageDecision::Reject(crate::stage::StageRejection::new(
                        403,
                        "fetch_metadata_same_site_denied",
                        "Same-site ambient-credential mutation denied",
                    ))
                }
                Some(_) => StageDecision::Continue(Box::new(input)),
            }
        })
    }
}

fn valid_fetch_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn header<'a>(input: &'a StageInput, name: &str) -> Option<&'a str> {
    input
        .request
        .headers
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizerFailureMode {
    FailOpen,
    FailClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleFailureKind {
    Panic,
    Cancelled,
    DeadlineExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalizerOutcome {
    Succeeded,
    Failed(LifecycleFailureKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizerEvent {
    pub stage: String,
    pub outcome: FinalizerOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardenedPipelineConfigError {
    pub code: &'static str,
}

struct HardenedStage {
    handler: Arc<dyn MiddlewareStageHandler>,
    finalizer_failure_mode: FinalizerFailureMode,
}

#[derive(Default)]
pub struct HardenedStagePipeline {
    stages: Vec<HardenedStage>,
}

#[derive(Debug, Clone)]
pub struct HardenedExecution {
    pub response: StageResponse,
    pub finalizers: Vec<FinalizerEvent>,
}

impl HardenedStagePipeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn try_with_stage(
        mut self,
        handler: Arc<dyn MiddlewareStageHandler>,
        finalizer_failure_mode: FinalizerFailureMode,
    ) -> Result<Self, HardenedPipelineConfigError> {
        if self.stages.len() >= MAX_HARDENED_STAGES {
            return Err(HardenedPipelineConfigError {
                code: "stage_count_exceeded",
            });
        }
        self.stages.push(HardenedStage {
            handler,
            finalizer_failure_mode,
        });
        Ok(self)
    }

    pub async fn execute<F, Fut>(&self, mut input: StageInput, handler: F) -> HardenedExecution
    where
        F: FnOnce(StageInput) -> Fut,
        Fut: Future<Output = StageResponse>,
    {
        let deadline = monotonic_deadline(input.context.deadline_unix_ms);
        if validate_stage_attributes(&input.attributes).is_err() {
            return HardenedExecution {
                response: sanitized_problem_response(
                    500,
                    "stage_attributes_invalid",
                    Some(&input.context.request_id),
                    &BTreeMap::new(),
                ),
                finalizers: Vec::new(),
            };
        }

        let mut entered = Vec::<(Arc<dyn MiddlewareStageHandler>, StageInput, FinalizerFailureMode)>::new();
        for stage in &self.stages {
            let received = input.clone();
            let stage_name = stage.handler.name().to_owned();
            let request = run_stage_request(Arc::clone(&stage.handler), input, deadline).await;
            match request {
                Ok(StageDecision::Continue(next)) => {
                    let next = *next;
                    entered.push((
                        Arc::clone(&stage.handler),
                        next.clone(),
                        stage.finalizer_failure_mode,
                    ));
                    if validate_stage_attributes(&next.attributes).is_err() {
                        return unwind_hardened(
                            entered,
                            sanitized_problem_response(
                                500,
                                "stage_attributes_invalid",
                                Some(&next.context.request_id),
                                &BTreeMap::new(),
                            ),
                            deadline,
                        )
                        .await;
                    }
                    input = next;
                }
                Ok(StageDecision::Respond(response)) => {
                    entered.push((
                        Arc::clone(&stage.handler),
                        received,
                        stage.finalizer_failure_mode,
                    ));
                    return unwind_hardened(entered, response, deadline).await;
                }
                Ok(StageDecision::Reject(rejection)) => {
                    entered.push((
                        Arc::clone(&stage.handler),
                        received.clone(),
                        stage.finalizer_failure_mode,
                    ));
                    let response = sanitized_problem_response(
                        rejection.status,
                        &rejection.code,
                        Some(&received.context.request_id),
                        &rejection.headers,
                    );
                    return unwind_hardened(entered, response, deadline).await;
                }
                Err(kind) => {
                    let response = sanitized_problem_response(
                        500,
                        match kind {
                            LifecycleFailureKind::Panic => "stage_panic",
                            LifecycleFailureKind::Cancelled => "stage_cancelled",
                            LifecycleFailureKind::DeadlineExceeded => "stage_deadline_exceeded",
                        },
                        Some(&received.context.request_id),
                        &BTreeMap::new(),
                    );
                    let mut result = unwind_hardened(entered, response, deadline).await;
                    result.finalizers.push(FinalizerEvent {
                        stage: stage_name,
                        outcome: FinalizerOutcome::Failed(kind),
                    });
                    return result;
                }
            }
        }

        let response = handler(input).await;
        unwind_hardened(entered, response, deadline).await
    }
}

async fn run_stage_request(
    stage: Arc<dyn MiddlewareStageHandler>,
    input: StageInput,
    deadline: Option<Instant>,
) -> Result<StageDecision, LifecycleFailureKind> {
    let mut task = tokio::spawn(async move { stage.request(input).await });
    join_with_deadline(&mut task, deadline).await
}

async fn run_finalizer(
    stage: Arc<dyn MiddlewareStageHandler>,
    input: StageInput,
    response: StageResponse,
    deadline: Option<Instant>,
) -> Result<StageResponse, LifecycleFailureKind> {
    let mut task = tokio::spawn(async move { stage.response(&input, response).await });
    join_with_deadline(&mut task, deadline).await
}

async fn join_with_deadline<T: Send + 'static>(
    task: &mut tokio::task::JoinHandle<T>,
    deadline: Option<Instant>,
) -> Result<T, LifecycleFailureKind> {
    let joined = match deadline {
        Some(deadline) => match timeout_at(deadline, &mut *task).await {
            Ok(result) => result,
            Err(_) => {
                task.abort();
                let _ = task.await;
                return Err(LifecycleFailureKind::DeadlineExceeded);
            }
        },
        None => task.await,
    };
    joined.map_err(classify_join_error)
}

fn classify_join_error(error: JoinError) -> LifecycleFailureKind {
    if error.is_panic() {
        LifecycleFailureKind::Panic
    } else {
        LifecycleFailureKind::Cancelled
    }
}

async fn unwind_hardened(
    entered: Vec<(Arc<dyn MiddlewareStageHandler>, StageInput, FinalizerFailureMode)>,
    mut response: StageResponse,
    deadline: Option<Instant>,
) -> HardenedExecution {
    let mut finalizers = Vec::with_capacity(entered.len());
    let mut terminal_failure = false;

    for (stage, input, failure_mode) in entered.into_iter().rev() {
        let stage_name = stage.name().to_owned();
        match run_finalizer(stage, input.clone(), response.clone(), deadline).await {
            Ok(next) => {
                finalizers.push(FinalizerEvent {
                    stage: stage_name,
                    outcome: FinalizerOutcome::Succeeded,
                });
                if !terminal_failure {
                    response = next;
                }
            }
            Err(kind) => {
                finalizers.push(FinalizerEvent {
                    stage: stage_name,
                    outcome: FinalizerOutcome::Failed(kind),
                });
                if failure_mode == FinalizerFailureMode::FailClosed && !terminal_failure {
                    response = sanitized_problem_response(
                        500,
                        "finalizer_failure",
                        Some(&input.context.request_id),
                        &BTreeMap::new(),
                    );
                    terminal_failure = true;
                }
            }
        }
    }

    HardenedExecution {
        response,
        finalizers,
    }
}

fn monotonic_deadline(deadline_unix_ms: Option<u64>) -> Option<Instant> {
    let deadline_unix_ms = deadline_unix_ms?;
    let now_wall_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let now_wall_ms = u64::try_from(now_wall_ms).unwrap_or(u64::MAX);
    let remaining_ms = deadline_unix_ms.saturating_sub(now_wall_ms);
    Some(Instant::now() + Duration::from_millis(remaining_ms))
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BreakerKey {
    dependency_id: String,
    operation_class: String,
}

impl BreakerKey {
    pub fn new(
        dependency_id: impl Into<String>,
        operation_class: impl Into<String>,
    ) -> Result<Self, ResilienceConfigError> {
        let dependency_id = dependency_id.into();
        let operation_class = operation_class.into();
        if !valid_breaker_component(&dependency_id) || !valid_breaker_component(&operation_class) {
            return Err(ResilienceConfigError {
                code: "circuit_registry_key_invalid",
                message: "circuit registry keys require bounded configured identifiers",
            });
        }
        Ok(Self {
            dependency_id,
            operation_class,
        })
    }

    pub fn dependency_id(&self) -> &str {
        &self.dependency_id
    }

    pub fn operation_class(&self) -> &str {
        &self.operation_class
    }
}

fn valid_breaker_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_BREAKER_KEY_COMPONENT_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

#[derive(Debug, Clone, Copy)]
pub struct CircuitBreakerRegistryConfig {
    pub max_entries: usize,
    pub idle_ttl: Duration,
    pub breaker: CircuitBreakerConfig,
}

#[derive(Clone)]
pub struct CircuitBreakerRegistry {
    config: CircuitBreakerRegistryConfig,
    entries: Arc<Mutex<BTreeMap<BreakerKey, RegistryEntry>>>,
    evictions: Arc<AtomicU64>,
}

#[derive(Clone)]
struct RegistryEntry {
    breaker: CircuitBreaker,
    last_used: Instant,
}

impl CircuitBreakerRegistry {
    pub fn new(config: CircuitBreakerRegistryConfig) -> Result<Self, ResilienceConfigError> {
        if config.max_entries == 0 || config.max_entries > MAX_BREAKER_REGISTRY_ENTRIES {
            return Err(ResilienceConfigError {
                code: "circuit_registry_capacity_invalid",
                message: "circuit registry capacity must be within the supported bound",
            });
        }
        if config.idle_ttl.is_zero() {
            return Err(ResilienceConfigError {
                code: "circuit_registry_ttl_invalid",
                message: "circuit registry idle TTL must be positive",
            });
        }
        CircuitBreaker::new(config.breaker)?;
        Ok(Self {
            config,
            entries: Arc::new(Mutex::new(BTreeMap::new())),
            evictions: Arc::new(AtomicU64::new(0)),
        })
    }

    pub async fn get_or_insert(&self, key: BreakerKey) -> Result<CircuitBreaker, ResilienceConfigError> {
        let now = Instant::now();
        let mut entries = self.entries.lock().await;
        let before = entries.len();
        entries.retain(|_, entry| now.duration_since(entry.last_used) < self.config.idle_ttl);
        let expired = before.saturating_sub(entries.len());
        if expired > 0 {
            self.evictions
                .fetch_add(u64::try_from(expired).unwrap_or(u64::MAX), Ordering::Relaxed);
        }

        if let Some(entry) = entries.get_mut(&key) {
            entry.last_used = now;
            return Ok(entry.breaker.clone());
        }

        if entries.len() >= self.config.max_entries
            && let Some(victim) = entries
                .iter()
                .min_by(|(left_key, left), (right_key, right)| {
                    left.last_used
                        .cmp(&right.last_used)
                        .then_with(|| left_key.cmp(right_key))
                })
                .map(|(key, _)| key.clone())
        {
            entries.remove(&victim);
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }

        let breaker = CircuitBreaker::new(self.config.breaker)?;
        entries.insert(
            key,
            RegistryEntry {
                breaker: breaker.clone(),
                last_used: now,
            },
        );
        Ok(breaker)
    }

    pub async fn len(&self) -> usize {
        self.entries.lock().await.len()
    }

    pub fn eviction_count(&self) -> u64 {
        self.evictions.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        panic::AssertUnwindSafe,
        pin::Pin,
        sync::{Arc, atomic::AtomicUsize},
    };

    use serde_json::json;

    use super::*;
    use crate::{context::RequestContext, integrations::RequestMetadata};

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    #[test]
    fn caps_raw_header_count() {
        let headers = (0..=MAX_RAW_HEADERS)
            .map(|index| pair(&format!("x-test-{index}"), "v"))
            .collect::<Vec<_>>();
        assert_eq!(
            admit_raw_headers(&headers).unwrap_err().code,
            "header_count_exceeded"
        );
    }

    #[test]
    fn caps_header_name_bytes() {
        let name = format!("x{}", "a".repeat(MAX_HEADER_NAME_BYTES));
        assert_eq!(
            admit_raw_headers(&[pair(&name, "v")]).unwrap_err().code,
            "header_name_size_invalid"
        );
    }

    #[test]
    fn caps_header_value_bytes() {
        let value = "v".repeat(MAX_HEADER_VALUE_BYTES + 1);
        assert_eq!(
            admit_raw_headers(&[pair("x-test", &value)])
                .unwrap_err()
                .code,
            "header_value_size_exceeded"
        );
    }

    #[test]
    fn caps_aggregate_header_bytes() {
        let value = "v".repeat(MAX_HEADER_VALUE_BYTES);
        let headers = (0..9)
            .map(|index| pair(&format!("x-list-{index}"), &value))
            .collect::<Vec<_>>();
        assert_eq!(
            admit_raw_headers(&headers).unwrap_err().code,
            "header_bytes_exceeded"
        );
    }

    #[test]
    fn rejects_header_control_bytes() {
        assert_eq!(
            admit_raw_headers(&[pair("x-test", "safe\r\ninjected: yes")])
                .unwrap_err()
                .code,
            "header_value_invalid"
        );
    }

    #[test]
    fn canonicalizes_header_names_to_lowercase_after_admission() {
        let admitted = admit_raw_headers(&[pair("X-ORES-Request-ID", "r1")]).unwrap();
        assert_eq!(admitted.get("x-ores-request-id").map(String::as_str), Some("r1"));
        assert!(!admitted.contains_key("X-ORES-Request-ID"));
    }

    #[test]
    fn rejects_duplicate_authorization() {
        assert_eq!(
            admit_raw_headers(&[
                pair("Authorization", "Bearer one"),
                pair("authorization", "Bearer two"),
            ])
            .unwrap_err()
            .code,
            "duplicate_header_forbidden"
        );
    }

    #[test]
    fn rejects_duplicate_cookie() {
        assert_eq!(
            admit_raw_headers(&[pair("cookie", "a=1"), pair("Cookie", "b=2")])
                .unwrap_err()
                .code,
            "duplicate_header_forbidden"
        );
    }

    #[test]
    fn rejects_transfer_encoding_content_length_ambiguity() {
        assert_eq!(
            admit_raw_headers(&[
                pair("content-length", "4"),
                pair("transfer-encoding", "chunked"),
            ])
            .unwrap_err()
            .code,
            "transfer_length_ambiguous"
        );
    }

    #[test]
    fn joins_only_explicit_list_headers() {
        let admitted = admit_raw_headers(&[
            pair("Accept", "application/json"),
            pair("accept", "application/problem+json"),
        ])
        .unwrap();
        assert_eq!(
            admitted.get("accept").map(String::as_str),
            Some("application/json, application/problem+json")
        );
        assert_eq!(
            admit_raw_headers(&[pair("x-custom", "a"), pair("x-custom", "b")])
                .unwrap_err()
                .code,
            "duplicate_header_ambiguous"
        );
    }

    #[test]
    fn caps_stage_attribute_count() {
        let attributes = (0..=MAX_STAGE_ATTRIBUTES)
            .map(|index| (format!("key_{index}"), json!(index)))
            .collect();
        assert_eq!(
            validate_stage_attributes(&attributes).unwrap_err().code,
            "stage_attribute_count_exceeded"
        );
    }

    #[test]
    fn caps_stage_attribute_key_and_value_sizes() {
        let invalid_key = BTreeMap::from([("A".to_owned(), json!(1))]);
        assert_eq!(
            validate_stage_attributes(&invalid_key).unwrap_err().code,
            "stage_attribute_key_invalid"
        );
        let oversized = BTreeMap::from([(
            "payload".to_owned(),
            json!("x".repeat(MAX_STAGE_ATTRIBUTE_VALUE_BYTES + 1)),
        )]);
        assert_eq!(
            validate_stage_attributes(&oversized).unwrap_err().code,
            "stage_attribute_value_exceeded"
        );
    }

    #[test]
    fn sanitized_problem_never_reflects_raw_reason_or_unsafe_headers() {
        let headers = BTreeMap::from([
            ("retry-after".to_owned(), "5".to_owned()),
            ("set-cookie".to_owned(), "secret=1".to_owned()),
            ("x-request-id".to_owned(), "r1\r\nevil: yes".to_owned()),
        ]);
        let response = sanitized_problem_response(
            500,
            "SQL ERROR bearer secret should not appear",
            Some("req-1"),
            &headers,
        );
        let body = String::from_utf8(response.body).unwrap();
        assert!(!body.contains("SQL ERROR"));
        assert!(!body.contains("bearer"));
        assert!(!response.headers.contains_key("set-cookie"));
        assert!(!response.headers.contains_key("x-request-id"));
        assert_eq!(response.headers.get("retry-after").map(String::as_str), Some("5"));
    }

    fn stage_input(method: &str, headers: &[(&str, &str)]) -> StageInput {
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

    async fn run_fetch(policy: FetchMetadataPolicy, input: StageInput) -> StageDecision {
        FetchMetadataStage::new(policy).request(input).await
    }

    #[tokio::test]
    async fn rejects_malformed_fetch_metadata_tokens() {
        let decision = run_fetch(
            FetchMetadataPolicy::default(),
            stage_input("GET", &[("sec-fetch-site", "CROSS SITE")]),
        )
        .await;
        assert!(matches!(decision, StageDecision::Reject(_)));
    }

    #[tokio::test]
    async fn rejects_cross_site_ambient_credential_mutation() {
        let decision = run_fetch(
            FetchMetadataPolicy::default(),
            stage_input(
                "POST",
                &[("cookie", "session=s1"), ("sec-fetch-site", "cross-site")],
            ),
        )
        .await;
        assert!(matches!(decision, StageDecision::Reject(_)));
    }

    #[tokio::test]
    async fn missing_fetch_metadata_has_explicit_compatibility_mode() {
        let allowed = run_fetch(
            FetchMetadataPolicy::default(),
            stage_input("POST", &[("cookie", "session=s1")]),
        )
        .await;
        assert!(matches!(allowed, StageDecision::Continue(_)));

        let denied = run_fetch(
            FetchMetadataPolicy {
                allow_missing: false,
                ..FetchMetadataPolicy::default()
            },
            stage_input("POST", &[("cookie", "session=s1")]),
        )
        .await;
        assert!(matches!(denied, StageDecision::Reject(_)));
    }

    struct PassStage(&'static str);

    impl MiddlewareStageHandler for PassStage {
        fn name(&self) -> &'static str {
            self.0
        }

        fn request<'a>(
            &'a self,
            input: StageInput,
        ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
            Box::pin(async move { StageDecision::Continue(Box::new(input)) })
        }
    }

    struct PanicFinalizer;

    impl MiddlewareStageHandler for PanicFinalizer {
        fn name(&self) -> &'static str {
            "panic-finalizer"
        }

        fn request<'a>(
            &'a self,
            input: StageInput,
        ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
            Box::pin(async move { StageDecision::Continue(Box::new(input)) })
        }

        fn response<'a>(
            &'a self,
            _input: &'a StageInput,
            _response: StageResponse,
        ) -> Pin<Box<dyn Future<Output = StageResponse> + Send + 'a>> {
            Box::pin(async move { panic!("secret panic detail") })
        }
    }

    struct SlowFinalizer;

    impl MiddlewareStageHandler for SlowFinalizer {
        fn name(&self) -> &'static str {
            "slow-finalizer"
        }

        fn request<'a>(
            &'a self,
            input: StageInput,
        ) -> Pin<Box<dyn Future<Output = StageDecision> + Send + 'a>> {
            Box::pin(async move { StageDecision::Continue(Box::new(input)) })
        }

        fn response<'a>(
            &'a self,
            _input: &'a StageInput,
            response: StageResponse,
        ) -> Pin<Box<dyn Future<Output = StageResponse> + Send + 'a>> {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_secs(60)).await;
                response
            })
        }
    }

    #[tokio::test]
    async fn isolates_finalizer_panic_and_keeps_problem_sanitized() {
        let pipeline = HardenedStagePipeline::new()
            .try_with_stage(Arc::new(PanicFinalizer), FinalizerFailureMode::FailClosed)
            .unwrap();
        let execution = pipeline
            .execute(stage_input("GET", &[]), |_| async { StageResponse::empty(200) })
            .await;
        assert_eq!(execution.response.status, 500);
        assert_eq!(
            execution.finalizers[0].outcome,
            FinalizerOutcome::Failed(LifecycleFailureKind::Panic)
        );
        assert!(!String::from_utf8_lossy(&execution.response.body).contains("secret panic"));
    }

    #[tokio::test(start_paused = true)]
    async fn finalizer_deadline_aborts_hung_task() {
        let now_ms = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        )
        .unwrap_or(u64::MAX);
        let mut input = stage_input("GET", &[]);
        input.context.deadline_unix_ms = Some(now_ms + 10);
        let pipeline = HardenedStagePipeline::new()
            .try_with_stage(Arc::new(SlowFinalizer), FinalizerFailureMode::FailClosed)
            .unwrap();
        let execution = pipeline
            .execute(input, |_| async { StageResponse::empty(200) })
            .await;
        assert_eq!(execution.response.status, 500);
        assert_eq!(
            execution.finalizers[0].outcome,
            FinalizerOutcome::Failed(LifecycleFailureKind::DeadlineExceeded)
        );
    }

    #[test]
    fn rejects_more_than_bounded_stage_count() {
        let mut pipeline = HardenedStagePipeline::new();
        for _ in 0..MAX_HARDENED_STAGES {
            pipeline = pipeline
                .try_with_stage(Arc::new(PassStage("pass")), FinalizerFailureMode::FailOpen)
                .unwrap();
        }
        assert_eq!(
            pipeline
                .try_with_stage(Arc::new(PassStage("overflow")), FinalizerFailureMode::FailOpen)
                .unwrap_err()
                .code,
            "stage_count_exceeded"
        );
    }

    fn registry_config(max_entries: usize, ttl: Duration) -> CircuitBreakerRegistryConfig {
        CircuitBreakerRegistryConfig {
            max_entries,
            idle_ttl: ttl,
            breaker: CircuitBreakerConfig {
                failure_threshold: 2,
                open_for: Duration::from_secs(1),
            },
        }
    }

    #[test]
    fn breaker_keys_are_bounded_configured_identifiers() {
        assert!(BreakerKey::new("shared-auth", "verify").is_ok());
        assert_eq!(
            BreakerKey::new("https://example.test/path?tenant=attacker", "verify")
                .unwrap_err()
                .code,
            "circuit_registry_key_invalid"
        );
        assert_eq!(
            BreakerKey::new("x".repeat(MAX_BREAKER_KEY_COMPONENT_BYTES + 1), "read")
                .unwrap_err()
                .code,
            "circuit_registry_key_invalid"
        );
    }

    #[tokio::test]
    async fn registry_evicts_deterministically_at_capacity() {
        let registry = CircuitBreakerRegistry::new(registry_config(2, Duration::from_secs(60)))
            .unwrap();
        let a = BreakerKey::new("auth", "verify").unwrap();
        let b = BreakerKey::new("cache", "read").unwrap();
        let c = BreakerKey::new("ledger", "write").unwrap();
        registry.get_or_insert(a.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;
        registry.get_or_insert(b).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;
        registry.get_or_insert(a).await.unwrap();
        registry.get_or_insert(c).await.unwrap();
        assert_eq!(registry.len().await, 2);
        assert_eq!(registry.eviction_count(), 1);
    }

    #[tokio::test]
    async fn registry_expiry_resets_state_and_counts_eviction() {
        let registry = CircuitBreakerRegistry::new(registry_config(2, Duration::from_millis(2)))
            .unwrap();
        let first = BreakerKey::new("auth", "verify").unwrap();
        registry.get_or_insert(first).await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        registry
            .get_or_insert(BreakerKey::new("cache", "read").unwrap())
            .await
            .unwrap();
        assert_eq!(registry.len().await, 1);
        assert_eq!(registry.eviction_count(), 1);
    }

    #[test]
    fn problem_serializer_survives_adversarial_inputs_without_panicking() {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            sanitized_problem_response(
                500,
                "\n\r\0 SQL postgres://user:secret@example Bearer.abc",
                Some("\r\n"),
                &BTreeMap::new(),
            )
        }));
        assert!(result.is_ok());
    }

    #[test]
    fn atomic_counter_type_is_lock_free_irrelevant_but_bounded_metric_is_numeric() {
        let metric = AtomicUsize::new(0);
        metric.fetch_add(1, Ordering::Relaxed);
        assert_eq!(metric.load(Ordering::Relaxed), 1);
    }
}

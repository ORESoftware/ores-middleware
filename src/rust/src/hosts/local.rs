use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};

use uuid::Uuid;

use crate::{
    ActiveRequest, MiddlewareStack,
    frameworks::streaming_response::response_headers,
    host_abi::{
        MIDDLEWARE_HOST_ABI_SCHEMA, MiddlewareHostAbiError, MiddlewareHostBeginResult,
        MiddlewareHostCapabilities, MiddlewareHostCompletionBoundary, MiddlewareHostDescriptor,
        MiddlewareHostExecutionModel, MiddlewareHostFinishRequest, MiddlewareHostFinishResult,
        MiddlewareHostOutcome, MiddlewareHostRequest, MiddlewareHostResponseHeadPhase,
        MiddlewareHostResponseHeadRequest, MiddlewareHostResponseHeadResult,
    },
};

const LOCAL_ADAPTER_ID: &str = "ores-middleware/local-process";

struct LocalActiveRequest {
    active: ActiveRequest,
    response_head_status: Option<u16>,
    response_head_available_ms: Option<u64>,
    response_head_committed_ms: Option<u64>,
}

impl LocalActiveRequest {
    fn new(active: ActiveRequest) -> Self {
        Self {
            active,
            response_head_status: None,
            response_head_available_ms: None,
            response_head_committed_ms: None,
        }
    }
}

/// Native/local lifecycle adapter for an isolated middleware deployment unit.
///
/// The adapter owns middleware session state and records response lifecycle
/// observations supplied by the actual local transport host. HTTP parsing,
/// sockets, stdio framing, UDS/TCP selection, child-process supervision, and
/// provider-specific transport remain outside this type. That keeps policy and
/// lifecycle state reusable by hosts that consume the same provider-neutral
/// `host_abi`.
#[derive(Clone)]
pub struct LocalMiddlewareHost {
    stack: MiddlewareStack,
    active: Arc<Mutex<BTreeMap<String, LocalActiveRequest>>>,
}

impl LocalMiddlewareHost {
    #[must_use]
    pub fn new(stack: MiddlewareStack) -> Self {
        Self {
            stack,
            active: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn descriptor(&self) -> Result<MiddlewareHostDescriptor, MiddlewareHostAbiError> {
        MiddlewareHostDescriptor::for_config(
            LOCAL_ADAPTER_ID,
            MiddlewareHostExecutionModel::LocalProcess,
            MiddlewareHostCapabilities::local_process(),
            self.stack.config(),
        )
    }

    /// Admit one normalized host request and retain the middleware lifecycle
    /// state under an opaque session id until `finish` is called.
    ///
    /// A successful permit includes the admitted normalized request plus
    /// middleware-owned response headers. Portable P2 adapters can therefore
    /// carry approved request mutations to P3 without manufacturing transport
    /// trust facts, while a streaming host can send its response head before
    /// the body completes.
    pub async fn begin(
        &self,
        request: MiddlewareHostRequest,
    ) -> Result<MiddlewareHostBeginResult, MiddlewareHostAbiError> {
        let admitted_request = request.clone();
        let request = request.into_request_metadata()?;
        match self.stack.begin(request).await {
            Ok(active) => {
                let response_headers = response_headers(&self.stack, &active);
                let request_id = active.context.request_id.clone();
                let trace_id = active.context.trace_id.clone();
                let session_id = Uuid::new_v4().to_string();
                let previous = self
                    .lock_active()?
                    .insert(session_id.clone(), LocalActiveRequest::new(active));
                if previous.is_some() {
                    return Err(MiddlewareHostAbiError::new(
                        "session_collision",
                        "middleware host generated a duplicate active session id",
                    ));
                }
                let result = MiddlewareHostBeginResult::Permit {
                    schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                    session_id,
                    request_id,
                    trace_id,
                    request: admitted_request,
                    response_headers,
                };
                result.validate()?;
                Ok(result)
            }
            Err(error) => {
                let result = MiddlewareHostBeginResult::Reject {
                    schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                    status: error.status,
                    code: error.code.to_owned(),
                    message: error.message,
                    headers: error.headers,
                };
                result.validate()?;
                Ok(result)
            }
        }
    }

    /// Record a response-head observation made by the owning transport adapter.
    ///
    /// `available` must be recorded before `committed`; the status cannot change
    /// between phases. Repeated observations of the same phase are idempotent
    /// and return the first observed elapsed time.
    pub fn observe_response_head(
        &self,
        request: MiddlewareHostResponseHeadRequest,
    ) -> Result<MiddlewareHostResponseHeadResult, MiddlewareHostAbiError> {
        request.validate()?;
        let mut active = self.lock_active()?;
        let state = active.get_mut(&request.session_id).ok_or_else(|| {
            MiddlewareHostAbiError::new(
                "unknown_session",
                "middleware host session is unknown or already finalized",
            )
        })?;
        if state
            .response_head_status
            .is_some_and(|status| status != request.status)
        {
            return Err(MiddlewareHostAbiError::new(
                "response_status_changed",
                "middleware host response status changed between lifecycle observations",
            ));
        }
        state.response_head_status = Some(request.status);
        let now = elapsed_ms(state.active.started);
        let elapsed_ms = match request.phase {
            MiddlewareHostResponseHeadPhase::Available => {
                *state.response_head_available_ms.get_or_insert(now)
            }
            MiddlewareHostResponseHeadPhase::Committed => {
                if state.response_head_available_ms.is_none() {
                    return Err(MiddlewareHostAbiError::new(
                        "response_head_commit_before_available",
                        "middleware host cannot report response-head commit before availability",
                    ));
                }
                *state.response_head_committed_ms.get_or_insert(now)
            }
        };
        Ok(MiddlewareHostResponseHeadResult {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            status: request.status,
            phase: request.phase,
            elapsed_ms,
        })
    }

    /// Finalize exactly one admitted request.
    ///
    /// The session is removed from the host table before any async finalization
    /// work begins. Double-finish therefore fails closed and an abandoned
    /// caller cannot race a second completion against cleanup/telemetry.
    pub async fn finish(
        &self,
        request: MiddlewareHostFinishRequest,
    ) -> Result<MiddlewareHostFinishResult, MiddlewareHostAbiError> {
        request.validate()?;
        let state = self
            .lock_active()?
            .remove(&request.session_id)
            .ok_or_else(|| {
                MiddlewareHostAbiError::new(
                    "unknown_session",
                    "middleware host session is unknown or already finalized",
                )
            })?;
        if state
            .response_head_status
            .is_some_and(|status| status != request.status)
        {
            return Err(MiddlewareHostAbiError::new(
                "response_status_changed",
                "middleware host final status disagrees with the observed response head",
            ));
        }
        let started = state.active.started;
        let headers = self
            .stack
            .finish(state.active, request.status, request.response_bytes)
            .await;
        Ok(MiddlewareHostFinishResult {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            outcome: request.outcome,
            completion_boundary: request.completion_boundary,
            status: request.status,
            time_to_response_head_available_ms: state.response_head_available_ms,
            time_to_response_head_committed_ms: state.response_head_committed_ms,
            total_duration_ms: elapsed_ms(started),
            response_headers: headers,
        })
    }

    /// Number of currently admitted requests awaiting finalization.
    pub fn active_count(&self) -> Result<usize, MiddlewareHostAbiError> {
        Ok(self.lock_active()?.len())
    }

    fn lock_active(
        &self,
    ) -> Result<MutexGuard<'_, BTreeMap<String, LocalActiveRequest>>, MiddlewareHostAbiError> {
        self.active.lock().map_err(|_| {
            MiddlewareHostAbiError::new(
                "host_state_poisoned",
                "middleware host lifecycle state is unavailable after a panic",
            )
        })
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        default_config,
        host_abi::{MiddlewareHostOutcome, MiddlewareHostRequest},
    };

    fn host() -> LocalMiddlewareHost {
        let mut config = default_config("local-host-test");
        config.settings.rate_limit.enabled = false;
        LocalMiddlewareHost::new(MiddlewareStack::new(config).expect("stack"))
    }

    fn secure_request(path: &str) -> MiddlewareHostRequest {
        let mut request = MiddlewareHostRequest::new("GET", path);
        request.trusted_transport_secure = true;
        request
    }

    #[tokio::test]
    async fn permit_keeps_request_active_until_explicit_finish() {
        let host = host();
        let result = host.begin(secure_request("/stream")).await.expect("begin");
        let MiddlewareHostBeginResult::Permit {
            session_id,
            request,
            response_headers,
            ..
        } = result
        else {
            panic!("expected permit");
        };
        assert_eq!(request.path, "/stream");
        assert_eq!(host.active_count().unwrap(), 1);
        assert!(response_headers.contains_key("x-content-type-options"));

        let available = host
            .observe_response_head(MiddlewareHostResponseHeadRequest {
                schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                session_id: session_id.clone(),
                status: 200,
                phase: MiddlewareHostResponseHeadPhase::Available,
            })
            .expect("head available");
        let committed = host
            .observe_response_head(MiddlewareHostResponseHeadRequest {
                schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                session_id: session_id.clone(),
                status: 200,
                phase: MiddlewareHostResponseHeadPhase::Committed,
            })
            .expect("head committed");
        assert!(committed.elapsed_ms >= available.elapsed_ms);

        let finish = host
            .finish(MiddlewareHostFinishRequest {
                schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                session_id,
                status: 200,
                response_bytes: None,
                outcome: MiddlewareHostOutcome::Completed,
                completion_boundary: MiddlewareHostCompletionBoundary::Transport,
            })
            .await
            .expect("finish");
        assert_eq!(finish.outcome, MiddlewareHostOutcome::Completed);
        assert_eq!(finish.status, 200);
        assert!(finish.time_to_response_head_available_ms.is_some());
        assert!(finish.time_to_response_head_committed_ms.is_some());
        assert_eq!(host.active_count().unwrap(), 0);
    }

    #[tokio::test]
    async fn commit_before_available_fails_closed() {
        let host = host();
        let result = host.begin(secure_request("/stream")).await.expect("begin");
        let MiddlewareHostBeginResult::Permit { session_id, .. } = result else {
            panic!("expected permit");
        };
        let error = host
            .observe_response_head(MiddlewareHostResponseHeadRequest {
                schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                session_id,
                status: 200,
                phase: MiddlewareHostResponseHeadPhase::Committed,
            })
            .expect_err("commit before availability");
        assert_eq!(error.code, "response_head_commit_before_available");
    }

    #[tokio::test]
    async fn disconnect_and_timeout_use_the_same_cleanup_boundary() {
        for (status, outcome) in [
            (499, MiddlewareHostOutcome::Disconnected),
            (504, MiddlewareHostOutcome::TimedOut),
        ] {
            let host = host();
            let result = host.begin(secure_request("/stream")).await.expect("begin");
            let MiddlewareHostBeginResult::Permit { session_id, .. } = result else {
                panic!("expected permit");
            };
            let finish = host
                .finish(MiddlewareHostFinishRequest {
                    schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                    session_id,
                    status,
                    response_bytes: None,
                    outcome,
                    completion_boundary: MiddlewareHostCompletionBoundary::Transport,
                })
                .await
                .expect("finish");
            assert_eq!(finish.outcome, outcome);
            assert_eq!(finish.completion_boundary, MiddlewareHostCompletionBoundary::Transport);
            assert_eq!(host.active_count().unwrap(), 0);
        }
    }

    #[tokio::test]
    async fn second_finish_fails_closed() {
        let host = host();
        let result = host.begin(secure_request("/")).await.expect("begin");
        let MiddlewareHostBeginResult::Permit { session_id, .. } = result else {
            panic!("expected permit");
        };
        let finish = MiddlewareHostFinishRequest {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            session_id,
            status: 200,
            response_bytes: Some(0),
            outcome: MiddlewareHostOutcome::Completed,
            completion_boundary: MiddlewareHostCompletionBoundary::BodyStream,
        };
        host.finish(finish.clone()).await.expect("first finish");
        let error = host.finish(finish).await.expect_err("double finish");
        assert_eq!(error.code, "unknown_session");
    }
}

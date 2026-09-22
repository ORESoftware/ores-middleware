use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
};

use uuid::Uuid;

use crate::{
    ActiveRequest, MiddlewareStack,
    frameworks::streaming_response::response_headers,
    host_abi::{
        MIDDLEWARE_HOST_ABI_SCHEMA, MiddlewareHostAbiError, MiddlewareHostBeginResult,
        MiddlewareHostCapabilities, MiddlewareHostDescriptor, MiddlewareHostExecutionModel,
        MiddlewareHostFinishRequest, MiddlewareHostFinishResult, MiddlewareHostRequest,
        MiddlewareHostResponseHeadPhase, MiddlewareHostResponseHeadRequest,
        MiddlewareHostResponseHeadResult,
    },
};

struct LocalHostSession {
    active: ActiveRequest,
    time_to_response_head_available_ms: Option<u64>,
    time_to_response_head_committed_ms: Option<u64>,
}

/// Native/local host adapter for an isolated middleware deployment unit.
///
/// The adapter deliberately owns only lifecycle/session state. HTTP parsing,
/// sockets, stdio framing, UDS/TCP selection, child-process supervision, and
/// provider-specific transport all belong outside this type. That keeps the
/// policy/runtime core reusable by edge hosts that consume the same `host_abi`.
#[derive(Clone)]
pub struct LocalMiddlewareHost {
    stack: MiddlewareStack,
    active: Arc<Mutex<BTreeMap<String, LocalHostSession>>>,
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
        MiddlewareHostDescriptor::for_adapter_config(
            "ores.local-process",
            MiddlewareHostExecutionModel::LocalProcess,
            MiddlewareHostCapabilities::local_process(),
            self.stack.config(),
        )
    }

    /// Admit one normalized host request and retain the middleware lifecycle
    /// state under an opaque session id until `finish` is called.
    ///
    /// A successful permit includes middleware-owned response headers so a
    /// streaming host can make the response head available before the body
    /// completes. Availability and actual transport commitment are recorded
    /// separately through `observe_response_head`.
    pub async fn begin(
        &self,
        request: MiddlewareHostRequest,
    ) -> Result<MiddlewareHostBeginResult, MiddlewareHostAbiError> {
        let request = request.into_request_metadata()?;
        match self.stack.begin(request).await {
            Ok(active) => {
                let response_headers = response_headers(&self.stack, &active);
                let request_id = active.context.request_id.clone();
                let trace_id = active.context.trace_id.clone();
                let session_id = Uuid::new_v4().to_string();
                let previous = self.lock_active()?.insert(
                    session_id.clone(),
                    LocalHostSession {
                        active,
                        time_to_response_head_available_ms: None,
                        time_to_response_head_committed_ms: None,
                    },
                );
                if previous.is_some() {
                    return Err(MiddlewareHostAbiError::new(
                        "session_collision",
                        "middleware host generated a duplicate active session id",
                    ));
                }
                Ok(MiddlewareHostBeginResult::Permit {
                    schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                    session_id,
                    request_id,
                    trace_id,
                    response_headers,
                })
            }
            Err(error) => Ok(MiddlewareHostBeginResult::Reject {
                schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                status: error.status,
                code: error.code.to_owned(),
                message: error.message,
                headers: error.headers,
            }),
        }
    }

    /// Record a truthful response-head observation for one admitted request.
    ///
    /// `available` means the status/headers exist and can be consumed by the
    /// downstream host. `committed` is stronger: the caller must have observed
    /// the downstream transport actually commit that response head. The local
    /// adapter refuses to fabricate `committed` before `available` was reported.
    pub fn observe_response_head(
        &self,
        request: MiddlewareHostResponseHeadRequest,
    ) -> Result<MiddlewareHostResponseHeadResult, MiddlewareHostAbiError> {
        request.validate()?;
        let mut active = self.lock_active()?;
        let session = active.get_mut(&request.session_id).ok_or_else(|| {
            MiddlewareHostAbiError::new(
                "unknown_session",
                "middleware host session is unknown or already finalized",
            )
        })?;
        let elapsed_ms = elapsed_ms(&session.active);

        let observed_ms = match request.phase {
            MiddlewareHostResponseHeadPhase::Available => *session
                .time_to_response_head_available_ms
                .get_or_insert(elapsed_ms),
            MiddlewareHostResponseHeadPhase::Committed => {
                if session.time_to_response_head_available_ms.is_none() {
                    return Err(MiddlewareHostAbiError::new(
                        "response_head_not_available",
                        "middleware host cannot record transport commitment before response-head availability",
                    ));
                }
                *session
                    .time_to_response_head_committed_ms
                    .get_or_insert(elapsed_ms)
            }
        };

        Ok(MiddlewareHostResponseHeadResult {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            status: request.status,
            phase: request.phase,
            elapsed_ms: observed_ms,
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
        let session = self
            .lock_active()?
            .remove(&request.session_id)
            .ok_or_else(|| {
                MiddlewareHostAbiError::new(
                    "unknown_session",
                    "middleware host session is unknown or already finalized",
                )
            })?;

        let total_duration_ms = elapsed_ms(&session.active);
        let headers = self
            .stack
            .finish(session.active, request.status, request.response_bytes)
            .await;
        Ok(MiddlewareHostFinishResult {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            outcome: request.outcome,
            completion_boundary: request.completion_boundary,
            status: request.status,
            time_to_response_head_available_ms: session.time_to_response_head_available_ms,
            time_to_response_head_committed_ms: session.time_to_response_head_committed_ms,
            total_duration_ms,
            response_headers: headers,
        })
    }

    /// Number of currently admitted requests awaiting finalization.
    pub fn active_count(&self) -> Result<usize, MiddlewareHostAbiError> {
        Ok(self.lock_active()?.len())
    }

    fn lock_active(
        &self,
    ) -> Result<MutexGuard<'_, BTreeMap<String, LocalHostSession>>, MiddlewareHostAbiError> {
        self.active.lock().map_err(|_| {
            MiddlewareHostAbiError::new(
                "host_state_poisoned",
                "middleware host lifecycle state is unavailable after a panic",
            )
        })
    }
}

fn elapsed_ms(active: &ActiveRequest) -> u64 {
    u64::try_from(active.started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        default_config,
        host_abi::{
            MiddlewareHostCompletionBoundary, MiddlewareHostOutcome, MiddlewareHostRequest,
            MiddlewareHostResponseHeadPhase, MiddlewareHostResponseHeadRequest,
        },
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
    async fn permit_tracks_head_observations_until_explicit_finish() {
        let host = host();
        let result = host.begin(secure_request("/stream")).await.expect("begin");
        let MiddlewareHostBeginResult::Permit {
            session_id,
            response_headers,
            ..
        } = result
        else {
            panic!("expected permit");
        };
        assert_eq!(host.active_count().unwrap(), 1);
        assert!(response_headers.contains_key("x-content-type-options"));

        let available = host
            .observe_response_head(MiddlewareHostResponseHeadRequest {
                schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                session_id: session_id.clone(),
                status: 200,
                phase: MiddlewareHostResponseHeadPhase::Available,
            })
            .expect("available");
        assert_eq!(available.phase, MiddlewareHostResponseHeadPhase::Available);

        let committed = host
            .observe_response_head(MiddlewareHostResponseHeadRequest {
                schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                session_id: session_id.clone(),
                status: 200,
                phase: MiddlewareHostResponseHeadPhase::Committed,
            })
            .expect("committed");
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
        assert_eq!(
            finish.completion_boundary,
            MiddlewareHostCompletionBoundary::Transport
        );
        assert!(finish.time_to_response_head_available_ms.is_some());
        assert!(finish.time_to_response_head_committed_ms.is_some());
        assert_eq!(host.active_count().unwrap(), 0);
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
            assert_eq!(host.active_count().unwrap(), 0);
        }
    }

    #[tokio::test]
    async fn committed_head_without_available_head_fails_closed() {
        let host = host();
        let result = host.begin(secure_request("/stream")).await.expect("begin");
        let MiddlewareHostBeginResult::Permit { session_id, .. } = result else {
            panic!("expected permit");
        };
        let error = host
            .observe_response_head(MiddlewareHostResponseHeadRequest {
                schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                session_id: session_id.clone(),
                status: 200,
                phase: MiddlewareHostResponseHeadPhase::Committed,
            })
            .expect_err("commit without availability must fail");
        assert_eq!(error.code, "response_head_not_available");
        assert_eq!(host.active_count().unwrap(), 1);

        host.finish(MiddlewareHostFinishRequest {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            session_id,
            status: 500,
            response_bytes: None,
            outcome: MiddlewareHostOutcome::ChildFailed,
            completion_boundary: MiddlewareHostCompletionBoundary::BodyStream,
        })
        .await
        .expect("cleanup");
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

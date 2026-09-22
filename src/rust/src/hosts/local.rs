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
        MiddlewareHostDescriptor, MiddlewareHostFinishRequest, MiddlewareHostFinishResult,
        MiddlewareHostKind, MiddlewareHostRequest,
    },
};

/// Native/local host adapter for an isolated middleware deployment unit.
///
/// The adapter deliberately owns only lifecycle/session state. HTTP parsing,
/// sockets, stdio framing, UDS/TCP selection, child-process supervision, and
/// provider-specific transport all belong outside this type. That keeps the
/// policy/runtime core reusable by edge hosts that consume the same `host_abi`.
#[derive(Clone)]
pub struct LocalMiddlewareHost {
    stack: MiddlewareStack,
    active: Arc<Mutex<BTreeMap<String, ActiveRequest>>>,
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
        MiddlewareHostDescriptor::for_config(MiddlewareHostKind::LocalProcess, self.stack.config())
    }

    /// Admit one normalized host request and retain the middleware lifecycle
    /// state under an opaque session id until `finish` is called.
    ///
    /// A successful permit includes middleware-owned response headers so a
    /// streaming host can send its response head before the body completes.
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
                let previous = self.lock_active()?.insert(session_id.clone(), active);
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
        let active = self
            .lock_active()?
            .remove(&request.session_id)
            .ok_or_else(|| {
                MiddlewareHostAbiError::new(
                    "unknown_session",
                    "middleware host session is unknown or already finalized",
                )
            })?;
        let headers = self
            .stack
            .finish(active, request.status, request.response_bytes)
            .await;
        Ok(MiddlewareHostFinishResult {
            schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
            outcome: request.outcome,
            response_headers: headers,
        })
    }

    /// Number of currently admitted requests awaiting finalization.
    pub fn active_count(&self) -> Result<usize, MiddlewareHostAbiError> {
        Ok(self.lock_active()?.len())
    }

    fn lock_active(
        &self,
    ) -> Result<MutexGuard<'_, BTreeMap<String, ActiveRequest>>, MiddlewareHostAbiError> {
        self.active.lock().map_err(|_| {
            MiddlewareHostAbiError::new(
                "host_state_poisoned",
                "middleware host lifecycle state is unavailable after a panic",
            )
        })
    }
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
            response_headers,
            ..
        } = result
        else {
            panic!("expected permit");
        };
        assert_eq!(host.active_count().unwrap(), 1);
        assert!(response_headers.contains_key("x-content-type-options"));

        let finish = host
            .finish(MiddlewareHostFinishRequest {
                schema: MIDDLEWARE_HOST_ABI_SCHEMA.to_owned(),
                session_id,
                status: 200,
                response_bytes: None,
                outcome: MiddlewareHostOutcome::Completed,
            })
            .await
            .expect("finish");
        assert_eq!(finish.outcome, MiddlewareHostOutcome::Completed);
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
                })
                .await
                .expect("finish");
            assert_eq!(finish.outcome, outcome);
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
        };
        host.finish(finish.clone()).await.expect("first finish");
        let error = host.finish(finish).await.expect_err("double finish");
        assert_eq!(error.code, "unknown_session");
    }
}

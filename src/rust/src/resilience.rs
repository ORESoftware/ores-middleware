use std::{sync::Arc, time::Duration};

use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

#[derive(Debug, Clone, Copy)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub open_for: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResilienceConfigError {
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitAdmission {
    Allowed,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitStateSnapshot {
    Closed { consecutive_failures: u32 },
    Open,
    HalfOpen,
}

#[derive(Debug)]
enum CircuitState {
    Closed { consecutive_failures: u32 },
    Open { opened_at: Instant },
    HalfOpen { probe_in_flight: bool },
}

#[derive(Clone)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: Arc<Mutex<CircuitState>>,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Result<Self, ResilienceConfigError> {
        if config.failure_threshold == 0 {
            return Err(ResilienceConfigError {
                code: "circuit_failure_threshold_invalid",
                message: "circuit failure threshold must be positive",
            });
        }
        if config.open_for.is_zero() {
            return Err(ResilienceConfigError {
                code: "circuit_open_duration_invalid",
                message: "circuit open duration must be positive",
            });
        }
        Ok(Self {
            config,
            state: Arc::new(Mutex::new(CircuitState::Closed {
                consecutive_failures: 0,
            })),
        })
    }

    pub async fn admit(&self) -> CircuitAdmission {
        let mut state = self.state.lock().await;
        match &mut *state {
            CircuitState::Closed { .. } => CircuitAdmission::Allowed,
            CircuitState::Open { opened_at }
                if opened_at.elapsed() >= self.config.open_for =>
            {
                *state = CircuitState::HalfOpen {
                    probe_in_flight: true,
                };
                CircuitAdmission::Allowed
            }
            CircuitState::Open { .. } => CircuitAdmission::Rejected,
            CircuitState::HalfOpen { probe_in_flight } if !*probe_in_flight => {
                *probe_in_flight = true;
                CircuitAdmission::Allowed
            }
            CircuitState::HalfOpen { .. } => CircuitAdmission::Rejected,
        }
    }

    pub async fn record_success(&self) {
        *self.state.lock().await = CircuitState::Closed {
            consecutive_failures: 0,
        };
    }

    pub async fn record_failure(&self) {
        let mut state = self.state.lock().await;
        match &mut *state {
            CircuitState::Closed {
                consecutive_failures,
            } => {
                *consecutive_failures = consecutive_failures.saturating_add(1);
                if *consecutive_failures >= self.config.failure_threshold {
                    *state = CircuitState::Open {
                        opened_at: Instant::now(),
                    };
                }
            }
            CircuitState::Open { .. } => {}
            CircuitState::HalfOpen { .. } => {
                *state = CircuitState::Open {
                    opened_at: Instant::now(),
                };
            }
        }
    }

    pub async fn cancel_probe(&self) {
        let mut state = self.state.lock().await;
        if let CircuitState::HalfOpen { probe_in_flight } = &mut *state {
            *probe_in_flight = false;
        }
    }

    pub async fn state(&self) -> CircuitStateSnapshot {
        match &*self.state.lock().await {
            CircuitState::Closed {
                consecutive_failures,
            } => CircuitStateSnapshot::Closed {
                consecutive_failures: *consecutive_failures,
            },
            CircuitState::Open { .. } => CircuitStateSnapshot::Open,
            CircuitState::HalfOpen { .. } => CircuitStateSnapshot::HalfOpen,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkheadRejected;

#[derive(Clone)]
pub struct Bulkhead {
    semaphore: Arc<Semaphore>,
}

impl Bulkhead {
    pub fn new(max_in_flight: usize) -> Result<Self, ResilienceConfigError> {
        if max_in_flight == 0 {
            return Err(ResilienceConfigError {
                code: "bulkhead_capacity_invalid",
                message: "bulkhead capacity must be positive",
            });
        }
        Ok(Self {
            semaphore: Arc::new(Semaphore::new(max_in_flight)),
        })
    }

    pub fn try_acquire(&self) -> Result<OwnedSemaphorePermit, BulkheadRejected> {
        Arc::clone(&self.semaphore)
            .try_acquire_owned()
            .map_err(|_| BulkheadRejected)
    }

    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaker(open_for: Duration) -> CircuitBreaker {
        CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            open_for,
        })
        .expect("valid breaker")
    }

    #[tokio::test]
    async fn circuit_opens_after_threshold_and_rejects_new_work() {
        let breaker = breaker(Duration::from_secs(30));
        assert_eq!(breaker.admit().await, CircuitAdmission::Allowed);
        breaker.record_failure().await;
        assert_eq!(
            breaker.state().await,
            CircuitStateSnapshot::Closed {
                consecutive_failures: 1
            }
        );
        breaker.record_failure().await;
        assert_eq!(breaker.state().await, CircuitStateSnapshot::Open);
        assert_eq!(breaker.admit().await, CircuitAdmission::Rejected);
    }

    #[tokio::test]
    async fn half_open_allows_one_probe_and_success_closes() {
        let breaker = breaker(Duration::from_millis(2));
        breaker.record_failure().await;
        breaker.record_failure().await;
        tokio::time::sleep(Duration::from_millis(5)).await;

        assert_eq!(breaker.admit().await, CircuitAdmission::Allowed);
        assert_eq!(breaker.state().await, CircuitStateSnapshot::HalfOpen);
        assert_eq!(breaker.admit().await, CircuitAdmission::Rejected);
        breaker.record_success().await;
        assert_eq!(
            breaker.state().await,
            CircuitStateSnapshot::Closed {
                consecutive_failures: 0
            }
        );
    }

    #[tokio::test]
    async fn failed_half_open_probe_reopens_the_circuit() {
        let breaker = breaker(Duration::from_millis(2));
        breaker.record_failure().await;
        breaker.record_failure().await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(breaker.admit().await, CircuitAdmission::Allowed);
        breaker.record_failure().await;
        assert_eq!(breaker.state().await, CircuitStateSnapshot::Open);
    }

    #[test]
    fn bulkhead_rejects_when_capacity_is_exhausted_and_recovers_on_drop() {
        let bulkhead = Bulkhead::new(1).expect("valid bulkhead");
        let permit = bulkhead.try_acquire().expect("first permit");
        assert_eq!(bulkhead.available_permits(), 0);
        assert!(matches!(bulkhead.try_acquire(), Err(BulkheadRejected)));
        drop(permit);
        assert_eq!(bulkhead.available_permits(), 1);
        assert!(bulkhead.try_acquire().is_ok());
    }

    #[test]
    fn zero_capacity_resilience_guards_are_rejected() {
        assert_eq!(
            Bulkhead::new(0).err().map(|error| error.code),
            Some("bulkhead_capacity_invalid")
        );
        assert_eq!(
            CircuitBreaker::new(CircuitBreakerConfig {
                failure_threshold: 0,
                open_for: Duration::from_secs(1),
            })
            .err()
            .map(|error| error.code),
            Some("circuit_failure_threshold_invalid")
        );
    }
}

# Graceful shutdown and request draining

`ores-middleware` owns the reusable request-admission and in-flight drain primitive. Consumer servers still own process signal policy, listener lifecycle, middleware ordering, telemetry flushing, and final process termination.

## Canonical sequence

1. Construct one `ShutdownCoordinator` for the server. The default drain deadline is five seconds.
2. Put `frameworks::axum_shutdown::admit_during_shutdown` at the outer request-admission boundary for Axum servers, or call `ShutdownCoordinator::begin_request()` at the equivalent boundary in another framework.
3. On the first actual shutdown trigger, call `start_draining()` and stop accepting new connections at the listener, ingress, or load-balancer boundary.
4. Requests already holding a `DrainGuard` may finish normally. Requests that race with the drain transition, including requests on existing keep-alive connections, fail closed with HTTP `503 Service Unavailable`, `Connection: close`, and `Retry-After`.
5. Await `ShutdownCoordinator::drain()`. If all existing requests complete before the deadline it returns `DrainOutcome::Drained`.
6. If the five-second deadline expires, the coordinator transitions to `ShutdownPhase::Forced` and returns `DrainOutcome::TimedOut`; cancel or abort remaining request/connection tasks at that point.
7. A second shutdown trigger may call `force_shutdown()` immediately rather than waiting for the deadline.
8. Flush OTel/logging and close external resources after the drain outcome is known, then terminate the process.

The listener should stop accepting first. The middleware admission guard is still required because connection accept and application dispatch are not one atomic operation, and HTTP keep-alive connections can already be inside the process when the listener closes.

## Signal and TTY ownership

This crate deliberately does **not** install global SIGINT/SIGTERM handlers or read stdin. That behavior is process-specific and belongs in the CLI/process lifecycle layer (for ORE CLI programs, `ores-clis-core`). This keeps reusable middleware safe for libraries, tests, embedded runtimes, and servers whose supervisors deliver shutdown through another mechanism.

An interactive CLI may therefore keep the ORE terminal policy independently: SIGINT can warn that Ctrl-D performs the graceful shutdown while non-interactive SIGINT/SIGTERM can trigger it directly. Whichever component decides that shutdown is real should call `start_draining()` exactly once; a subsequent signal may call `force_shutdown()`.

## Axum example

```rust
use axum::{Router, middleware};
use ores_middleware::{ShutdownCoordinator, frameworks::axum_shutdown};

let shutdown = ShutdownCoordinator::default();
let app = Router::new()
    // routes...
    .route_layer(middleware::from_fn_with_state(
        axum_shutdown::ShutdownLayerState::new(shutdown.clone()),
        axum_shutdown::admit_during_shutdown,
    ));

// First real shutdown trigger:
if shutdown.start_draining() {
    // Stop accepting new connections at the server/listener boundary.
}

match shutdown.drain().await {
    ores_middleware::DrainOutcome::Drained { .. } => {}
    ores_middleware::DrainOutcome::TimedOut { .. }
    | ores_middleware::DrainOutcome::Forced { .. } => {
        // Abort/cancel remaining connection tasks.
    }
}

// Flush telemetry/logging and close resources before process exit.
```

## Status code

Draining is temporary server unavailability, so the reusable HTTP adapter uses `503`, not a 4xx client-error status. `Retry-After` gives callers a bounded retry hint and `Connection: close` prevents a draining HTTP/1.1 connection from being reused.

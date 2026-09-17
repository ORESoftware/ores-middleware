# Graceful shutdown and request draining

`ores-middleware` owns reusable application-request admission and in-flight drain accounting. The canonical process lifecycle policy remains in `ores-otel/ores.otel.log`; consumer servers own signal installation, listener and protocol shutdown, middleware ordering, telemetry/resource flushing, and final process termination.

## Canonical sequence

1. Construct one `ShutdownCoordinator` for the server. The default drain deadline is five seconds.
2. Put `frameworks::axum_shutdown::admit_during_shutdown` at the outer request-admission boundary for Axum servers, or call `ShutdownCoordinator::begin_request()` at the equivalent boundary in another framework.
3. On the first canonical shutdown transition, call `start_draining()` and stop accepting new connections at the listener, ingress, or load-balancer boundary.
4. Requests already holding a `DrainGuard` may finish normally. Requests that race with the drain transition, including requests on existing keep-alive connections, fail closed with the canonical HTTP `429 Too Many Requests` admission response and `Retry-After`.
5. HTTP/1.x adapters additionally send `Connection: close`. The generic rejection never carries this hop-by-hop field, and HTTP/2+ adapters must omit it.
6. Await `ShutdownCoordinator::drain()`. If all existing admitted handlers complete before the deadline it returns `DrainOutcome::Drained`.
7. If the five-second deadline expires, the coordinator transitions to `ShutdownPhase::Forced` and returns `DrainOutcome::TimedOut`; cancel or abort remaining request/connection tasks at that point.
8. A canonical force transition (for example Ctrl-D while draining or an allowed second signal) may call `force_shutdown()` immediately rather than waiting for the deadline.
9. Flush OTel/logging and close external resources after the transport and application drain outcome is known, then terminate the process.

The listener should stop accepting first. The middleware admission guard is still required because connection accept and application dispatch are not one atomic operation, and HTTP keep-alive connections can already be inside the process when the listener closes. Transport-level graceful shutdown remains responsible for active connection/stream lifetimes; this coordinator accounts only for admitted application-handler lifetimes and dispatch races.

## Ownership and signal policy

This crate deliberately does **not** install global SIGINT/SIGTERM handlers or read stdin. ORE process code should obtain the lifecycle decision from the canonical shutdown policy and project that decision into this request-drain adapter. `ores-clis-core` may own the terminal event capture, but it must not invent a competing HTTP lifecycle or response contract.

For interactive servers, first SIGINT starts the canonical drain immediately while also warning that Ctrl-D can force shutdown; repeated interactive SIGINT does not bypass the grace window. Non-interactive SIGINT/SIGTERM starts the drain directly. Consumers translate the resulting lifecycle decision into `start_draining()` or `force_shutdown()` here.

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

// When the canonical lifecycle enters Draining:
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

## Rejection contract

The shared ORES shutdown contract uses HTTP `429` while the process is deliberately refusing new application work during a bounded drain. The body uses the hardened bounded `application/problem+json` serializer with `Cache-Control: no-store` and the stable `service_draining` code. `Retry-After` is emitted as whole delay-seconds rounded up. `Connection: close` is transport-specific and is added only for HTTP/1.0 and HTTP/1.1; HTTP/2 and later protocols prohibit that connection-specific field.

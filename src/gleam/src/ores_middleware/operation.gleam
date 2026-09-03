import ores_middleware
import ores_middleware/otel

/// Network protocol owning the guarded operation.
pub type OperationTransport {
  Http
  Tcp
  WebSocket
}

/// Failure domain within the owning protocol.
pub type OperationScope {
  Request
  Connection
  Message
  Callback
}

/// Runs one protocol operation with request context and Erlang Logger process
/// metadata installed. Erlang throw/error/exit failures become a bounded error
/// result; the raw reason and stack are not copied into telemetry.
pub fn run(
  context: ores_middleware.RequestContext,
  transport: OperationTransport,
  scope: OperationScope,
  name: String,
  operation: fn() -> result,
) -> Result(result, String) {
  run_ffi(
    context,
    transport_name(transport),
    scope_name(scope),
    name,
    operation,
  )
}

/// Adds the canonical ores-otel log context around the same BEAM operation
/// boundary. File/module loggers then receive the pinned request identifiers.
pub fn run_with_logger(
  context: ores_middleware.RequestContext,
  request_logger: otel.RequestLogger,
  transport: OperationTransport,
  scope: OperationScope,
  name: String,
  operation: fn() -> result,
) -> Result(result, String) {
  otel.with_request_context(request_logger, fn() {
    run(context, transport, scope, name, operation)
  })
}

fn transport_name(transport: OperationTransport) -> String {
  case transport {
    Http -> "http"
    Tcp -> "tcp"
    WebSocket -> "websocket"
  }
}

fn scope_name(scope: OperationScope) -> String {
  case scope {
    Request -> "request"
    Connection -> "connection"
    Message -> "message"
    Callback -> "callback"
  }
}

@external(erlang, "ores_middleware_context_ffi", "run_operation")
fn run_ffi(
  context: ores_middleware.RequestContext,
  transport: String,
  scope: String,
  name: String,
  operation: fn() -> result,
) -> Result(result, String)

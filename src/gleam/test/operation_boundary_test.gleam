import gleam/dict
import gleeunit
import ores_middleware
import ores_middleware/context
import ores_middleware/operation

pub fn main() {
  gleeunit.main()
}

fn test_context() -> ores_middleware.RequestContext {
  ores_middleware.RequestContext(
    request_id: "request-42",
    trace_id: "0123456789abcdef0123456789abcdef",
    tenant_id: "tenant-7",
    user_id: "user-42",
    locale: "en-US",
    started_at_unix_ms: 1,
    deadline_unix_ms: 2,
    baggage: dict.from_list([#("otel.vendor", "test")]),
  )
}

pub fn failed_message_isolated_and_later_message_runs_test() {
  let request_context = test_context()
  let failed =
    operation.run(
      request_context,
      operation.WebSocket,
      operation.Message,
      "chat.message",
      raise,
    )
  assert failed == Error("operation_failed")
  assert context.current_request_id() == Error(Nil)

  let succeeded =
    operation.run(
      request_context,
      operation.WebSocket,
      operation.Message,
      "chat.message",
      fn() {
        assert context.current_request_id() == Ok("request-42")
        "ok"
      },
    )
  assert succeeded == Ok("ok")
  assert context.current_request_id() == Error(Nil)
}

@external(erlang, "operation_boundary_test_ffi", "raise")
fn raise() -> Nil

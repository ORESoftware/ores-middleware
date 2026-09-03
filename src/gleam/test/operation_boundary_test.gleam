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

fn anonymous_context() -> ores_middleware.RequestContext {
  ores_middleware.RequestContext(
    request_id: "request-anonymous",
    trace_id: "fedcba9876543210fedcba9876543210",
    tenant_id: "",
    user_id: "",
    locale: "en-US",
    started_at_unix_ms: 1,
    deadline_unix_ms: 2,
    baggage: dict.new(),
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

pub fn anonymous_inner_scope_replaces_and_restores_owned_metadata_test() {
  set_outer_metadata()

  let observed =
    operation.run(
      anonymous_context(),
      operation.Tcp,
      operation.Callback,
      "tcp.read",
      fn() {
        assert current_metadata_user_id() == ""
        assert current_metadata_marker() == "outer-marker"
        "inner-ok"
      },
    )

  assert observed == Ok("inner-ok")
  assert current_metadata_user_id() == "outer-user"
  assert current_metadata_marker() == "outer-marker"
  clear_metadata()
}

@external(erlang, "operation_boundary_test_ffi", "raise")
fn raise() -> Nil

@external(erlang, "operation_boundary_test_ffi", "set_outer_metadata")
fn set_outer_metadata() -> Nil

@external(erlang, "operation_boundary_test_ffi", "current_user_id")
fn current_metadata_user_id() -> String

@external(erlang, "operation_boundary_test_ffi", "current_marker")
fn current_metadata_marker() -> String

@external(erlang, "operation_boundary_test_ffi", "clear_metadata")
fn clear_metadata() -> Nil

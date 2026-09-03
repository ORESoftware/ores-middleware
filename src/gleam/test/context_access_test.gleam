import gleam/dict
import gleeunit
import ores_middleware
import ores_middleware/context

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

pub fn typed_context_accessors_test() {
  let request_context = test_context()
  let expected_trace_id = "0123456789abcdef0123456789abcdef"

  assert context.request_id(request_context) == "request-42"
  assert context.trace_id(request_context) == expected_trace_id
  assert context.user_id(request_context) == "user-42"
  assert context.logged_in_user_id(request_context) == "user-42"
  assert context.tenant_id(request_context) == "tenant-7"
}

pub fn ambient_accessors_are_process_scoped_test() {
  let expected_trace_id = "0123456789abcdef0123456789abcdef"
  assert context.current_request_id() == Error(Nil)

  ores_middleware.run_with_context(test_context(), fn() {
    assert context.current_request_id() == Ok("request-42")
    assert context.current_trace_id() == Ok(expected_trace_id)
    assert context.current_logged_in_user_id() == Ok("user-42")
    assert context.current_tenant_id() == Ok("tenant-7")
  })

  assert context.current_request_id() == Error(Nil)
}

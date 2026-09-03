import gleam/dict
import gleam/json
import gleam/option.{None, Some}
import gleam/string
import ores_middleware
import ores_middleware/otel
import oresoftware_next_loggers as log
import oresoftware_next_loggers_context as log_context

fn test_logger(transport: log.Transport) -> log.Logger {
  otel.options(
    "middleware-adversarial-test",
    "gleam",
    fn() { "record-1" },
    fn() { "2026-09-03T00:00:00Z" },
  )
  |> otel.new_logger(transport)
}

fn context(slot: String) -> ores_middleware.RequestContext {
  ores_middleware.RequestContext(
    request_id: "request-" <> slot,
    trace_id: "0123456789abcdef0123456789abcdef",
    tenant_id: "tenant-" <> slot,
    user_id: "user-" <> slot,
    locale: "en-US",
    started_at_unix_ms: 1,
    deadline_unix_ms: 2,
    baggage: dict.from_list([
      #("otel.slot", slot),
      #("authorization", "must-not-propagate"),
      #("cookie", "must-not-propagate"),
    ]),
  )
}

pub fn nested_log_context_restores_exact_outer_scope_test() {
  let logger = test_logger(log.noop_transport())
  let outer = otel.to_log_context(context("outer"))
  let inner = otel.to_log_context(context("inner"))

  assert log_context.current_context() == None
  log_context.with_context(outer, fn() {
    let assert Some(current_outer) = log_context.current_context()
    assert current_outer.routine_id == Some("request-outer")
    assert current_outer.logged_in_user
      == Some([#("id", json.string("user-outer"))])

    log_context.with_context(inner, fn() {
      let assert Some(current_inner) = log_context.current_context()
      assert current_inner.routine_id == Some("request-inner")
      assert current_inner.logged_in_user
        == Some([#("id", json.string("user-inner"))])

      let record =
        log_context.info(logger, "nested file logger", [])
        |> log.record
        |> log.record_to_string
      assert string.contains(record, "\"request.id\":\"request-inner\"")
      assert string.contains(record, "\"user.id\":\"user-inner\"")
      assert string.contains(record, "\"tenant.id\":\"tenant-inner\"")
      assert string.contains(record, "\"loggedInUser\":{\"id\":\"user-inner\"}")
      assert !string.contains(record, "must-not-propagate")
    })

    let assert Some(restored_outer) = log_context.current_context()
    assert restored_outer.routine_id == Some("request-outer")
    assert restored_outer.logged_in_user
      == Some([#("id", json.string("user-outer"))])
  })
  assert log_context.current_context() == None
  let _ = log.close(logger)
}

pub fn request_logger_remains_pinned_when_ambient_context_changes_test() {
  let logger = test_logger(log.noop_transport())
  let pinned = otel.request_logger(logger, context("pinned"))
  let other = otel.to_log_context(context("other"))

  log_context.with_context(other, fn() {
    let record =
      otel.warn(pinned, "pinned request logger", [])
      |> log.record
      |> log.record_to_string

    assert string.contains(record, "\"request.id\":\"request-pinned\"")
    assert string.contains(record, "\"user.id\":\"user-pinned\"")
    assert string.contains(record, "\"tenant.id\":\"tenant-pinned\"")
    assert string.contains(record, "\"loggedInUser\":{\"id\":\"user-pinned\"}")
    assert !string.contains(record, "request-other")
    assert !string.contains(record, "user-other")
  })

  assert log_context.current_context() == None
  let _ = log.close(logger)
}

pub fn failing_transport_does_not_replace_middleware_response_test() {
  let failing_transport =
    log.Transport(
      write: fn(_) { Error("sink unavailable") },
      flush: fn() { Ok(Nil) },
      flush_on_exit: fn(_) { Ok(Nil) },
      close: fn() { Ok(Nil) },
    )
  let logger = test_logger(failing_transport)
  let config0 = ores_middleware.default_config("middleware-adversarial-test")
  let config =
    ores_middleware.Config(
      ..config0,
      environment: ores_middleware.Test,
      require_https: False,
      rate_limit_enabled: False,
      idempotency_enabled: False,
      compression_enabled: False,
    )
  let hooks0 = ores_middleware.default_hooks()
  let hooks =
    ores_middleware.Hooks(
      ..hooks0,
      authenticate: fn(_, _) {
        Ok(ores_middleware.AuthDecision(
          user_id: "user-transport",
          tenant_id: "tenant-transport",
          baggage: dict.from_list([#("otel.test", "allowed")]),
        ))
      },
    )
  let assert Ok(middleware) = otel.create_middleware(config, hooks, logger)
  let request =
    ores_middleware.Request(
      method: "GET",
      path: "/failing-transport",
      scheme: "http",
      headers: dict.from_list([
        #("accept", "application/json"),
        #("x-request-id", "request-transport"),
        #(
          "traceparent",
          "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01",
        ),
      ]),
      body_size: 0,
      remote_ip: "127.0.0.1",
    )

  let response = middleware(request, fn(scoped_request, request_logger) {
    assert scoped_request.path == "/failing-transport"
    let assert Error("sink unavailable") =
      otel.info(request_logger, "inside failing transport", [])
      |> otel.send
    ores_middleware.Response(204, dict.new(), "")
  })

  assert response.status == 204
  assert ores_middleware.current_context() == Error(Nil)
  assert log_context.current_context() == None
  let _ = log.close(logger)
}

pub fn middleware_callback_receives_authenticated_pinned_logger_test() {
  let logger = test_logger(log.noop_transport())
  let config0 = ores_middleware.default_config("middleware-adversarial-test")
  let config =
    ores_middleware.Config(
      ..config0,
      environment: ores_middleware.Test,
      require_https: False,
      rate_limit_enabled: False,
      idempotency_enabled: False,
      compression_enabled: False,
    )
  let hooks0 = ores_middleware.default_hooks()
  let hooks =
    ores_middleware.Hooks(
      ..hooks0,
      authenticate: fn(_, _) {
        Ok(ores_middleware.AuthDecision(
          user_id: "user-callback",
          tenant_id: "tenant-callback",
          baggage: dict.from_list([
            #("otel.allowed", "yes"),
            #("authorization", "must-not-propagate"),
          ]),
        ))
      },
    )
  let assert Ok(middleware) = otel.create_middleware(config, hooks, logger)
  let request =
    ores_middleware.Request(
      method: "GET",
      path: "/callback",
      scheme: "http",
      headers: dict.from_list([#("x-request-id", "request-callback")]),
      body_size: 0,
      remote_ip: "127.0.0.1",
    )

  let response = middleware(request, fn(_, request_logger) {
    let record =
      otel.info(request_logger, "callback reached", [])
      |> log.record
      |> log.record_to_string
    assert string.contains(record, "\"request.id\":\"request-callback\"")
    assert string.contains(record, "\"user.id\":\"user-callback\"")
    assert string.contains(record, "\"tenant.id\":\"tenant-callback\"")
    assert string.contains(record, "otel.allowed")
    assert !string.contains(record, "authorization")
    ores_middleware.Response(202, dict.new(), "accepted")
  })

  assert response.status == 202
  assert log_context.current_context() == None
  let _ = log.close(logger)
}

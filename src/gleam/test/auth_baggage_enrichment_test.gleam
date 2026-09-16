import gleam/dict
import gleam/string
import ores_middleware
import ores_middleware/otel
import oresoftware_next_loggers as log

fn logger() -> log.Logger {
  otel.options(
    "auth-baggage-enrichment-test",
    "gleam",
    fn() { "record-auth-baggage" },
    fn() { "2026-09-16T00:00:00Z" },
  )
  |> otel.new_logger(log.noop_transport())
}

fn config(test_auth_bypass_enabled: Bool) -> ores_middleware.Config {
  let base = ores_middleware.default_config("auth-baggage-enrichment-test")
  ores_middleware.Config(
    ..base,
    environment: ores_middleware.Test,
    require_https: False,
    rate_limit_enabled: False,
    idempotency_enabled: False,
    compression_enabled: False,
    test_auth_bypass_enabled: test_auth_bypass_enabled,
  )
}

fn request(headers: dict.Dict(String, String)) -> ores_middleware.Request {
  ores_middleware.Request(
    method: "GET",
    path: "/auth-baggage",
    scheme: "http",
    headers: headers,
    body_size: 0,
    remote_ip: "127.0.0.1",
  )
}

pub fn auth_baggage_is_not_propagated_by_default_test() {
  let logger = logger()
  let hooks0 = ores_middleware.default_hooks()
  let hooks =
    ores_middleware.Hooks(..hooks0, authenticate: fn(_, _) {
      Ok(ores_middleware.AuthDecision(
        user_id: "user-default",
        tenant_id: "tenant-default",
        baggage: dict.from_list([
          #("otel.marker", "must-not-propagate"),
          #("authorization", "Bearer secret"),
        ]),
      ))
    })
  let assert Ok(middleware) =
    otel.create_middleware(config(False), hooks, logger)

  let response =
    middleware(request(dict.new()), fn(_, request_logger) {
      let record =
        otel.info(request_logger, "default baggage boundary", [])
        |> log.record
        |> log.record_to_string
      assert string.contains(record, "\"user.id\":\"user-default\"")
      assert string.contains(record, "\"tenant.id\":\"tenant-default\"")
      assert !string.contains(record, "must-not-propagate")
      assert !string.contains(record, "Bearer secret")
      ores_middleware.Response(204, dict.new(), "")
    })

  assert response.status == 204
  let _ = log.close(logger)
}

pub fn test_auth_bypass_uses_same_explicit_enrichment_boundary_test() {
  let logger = logger()
  let hooks0 = ores_middleware.default_hooks()
  let hooks =
    ores_middleware.Hooks(
      ..hooks0,
      resolve_test_identity: fn(_, _) {
        Ok(ores_middleware.AuthDecision(
          user_id: "user-bypass",
          tenant_id: "tenant-bypass",
          baggage: dict.from_list([
            #("otel.allowed", "bypass-visible"),
            #("authorization", "bypass-secret"),
          ]),
        ))
      },
      auth_baggage_enricher: fn(_, _, auth: ores_middleware.AuthDecision) {
        case dict.get(auth.baggage, "otel.allowed") {
          Ok(value) -> dict.from_list([#("otel.allowed", value)])
          Error(_) -> dict.new()
        }
      },
    )
  let headers =
    dict.from_list([
      #("x-test-auth-bypass", "true"),
      #("x-request-id", "bypass"),
    ])
  let assert Ok(middleware) =
    otel.create_middleware(config(True), hooks, logger)

  let response =
    middleware(request(headers), fn(_, request_logger) {
      let record =
        otel.info(request_logger, "bypass baggage boundary", [])
        |> log.record
        |> log.record_to_string
      assert string.contains(record, "\"user.id\":\"user-bypass\"")
      assert string.contains(record, "\"tenant.id\":\"tenant-bypass\"")
      assert string.contains(record, "bypass-visible")
      assert !string.contains(record, "bypass-secret")
      ores_middleware.Response(202, dict.new(), "")
    })

  assert response.status == 202
  let _ = log.close(logger)
}

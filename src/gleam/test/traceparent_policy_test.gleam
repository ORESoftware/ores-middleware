import gleam/dict
import gleam/string
import ores_middleware

const zero_trace_id = "00000000000000000000000000000000"

const valid_trace_id = "0123456789abcdef0123456789abcdef"

const valid_parent_span_id = "0123456789abcdef"

const valid_server_span_id = "fedcba9876543210"

fn test_config() -> ores_middleware.Config {
  let config = ores_middleware.default_config("traceparent-policy-test")
  ores_middleware.Config(
    ..config,
    environment: ores_middleware.Test,
    require_https: False,
    rate_limit_enabled: False,
    idempotency_enabled: False,
    compression_enabled: False,
  )
}

fn request(trace_id: String) -> ores_middleware.Request {
  ores_middleware.Request(
    method: "GET",
    path: "/trace",
    scheme: "http",
    headers: dict.from_list([
      #("accept", "application/json"),
      #(
        "traceparent",
        "00-" <> trace_id <> "-" <> valid_parent_span_id <> "-01",
      ),
    ]),
    body_size: 0,
    remote_ip: "127.0.0.1",
  )
}

fn middleware() -> ores_middleware.Middleware {
  let assert Ok(value) =
    ores_middleware.create_middleware(
      test_config(),
      ores_middleware.default_hooks(),
    )
  value
}

pub fn inbound_parent_is_not_relabelled_as_response_span_test() {
  let response =
    middleware()(request(valid_trace_id), fn(_) {
      ores_middleware.Response(204, dict.new(), "")
    })

  assert dict.get(response.headers, "traceparent") == Error(Nil)
}

pub fn only_valid_tracer_owned_response_traceparent_is_preserved_test() {
  let valid = "00-" <> valid_trace_id <> "-" <> valid_server_span_id <> "-01"
  let valid_response =
    middleware()(request(valid_trace_id), fn(_) {
      ores_middleware.Response(
        204,
        dict.from_list([#("traceparent", string.uppercase(valid))]),
        "",
      )
    })
  assert dict.get(valid_response.headers, "traceparent") == Ok(valid)

  let invalid_response =
    middleware()(request(valid_trace_id), fn(_) {
      ores_middleware.Response(
        204,
        dict.from_list([
          #("traceparent", "00-" <> valid_trace_id <> "-0000000000000000-01"),
        ]),
        "",
      )
    })
  assert dict.get(invalid_response.headers, "traceparent") == Error(Nil)
}

pub fn all_zero_inbound_trace_id_is_replaced_test() {
  let response =
    middleware()(request(zero_trace_id), fn(_) {
      let assert Ok(context) = ores_middleware.current_context()
      assert context.trace_id != zero_trace_id
      assert string.length(context.trace_id) == 32
      ores_middleware.Response(204, dict.new(), "")
    })

  assert response.status == 204
  assert dict.get(response.headers, "traceparent") == Error(Nil)
}

from pathlib import Path


def replace_once(path: str, before: str, after: str) -> None:
    target = Path(path)
    text = target.read_text(encoding="utf-8")
    count = text.count(before)
    if count != 1:
        raise RuntimeError(f"expected exactly one match in {path}, found {count}: {before[:120]!r}")
    target.write_text(text.replace(before, after, 1), encoding="utf-8")


replace_once(
    "src/gleam/src/ores_middleware.gleam",
    '''fn handle(
  config: Config,
  hooks: Hooks,
  request: Request,
  next: fn(Request) -> Response,
) -> Response {
  case request.body_size > config.max_body_bytes {
    True ->
      problem(413, "payload_too_large", "request body exceeds configured limit")
    False -> handle_transport(config, hooks, request, next)
  }
}

fn handle_transport(
  config: Config,
  hooks: Hooks,
  request: Request,
  next: fn(Request) -> Response,
) -> Response {
  case config.require_https && request.scheme != "https" {
    True -> problem(426, "https_required", "HTTPS is required")
    False -> {
      let now = system_time_ms()
      let context =
        RequestContext(
          request_id: value_or_new(header(
            request.headers,
            config.request_id_header,
          )),
          trace_id: value_or_new(
            trace_id(header(request.headers, config.trace_header)),
          ),
          tenant_id: "",
          user_id: "",
          locale: header(request.headers, "accept-language"),
          started_at_unix_ms: now,
          deadline_unix_ms: now + config.timeout_ms,
          baggage: dict.new(),
        )
      handle_policy(config, hooks, request, next, context)
    }
  }
}
''',
    '''fn handle(
  config: Config,
  hooks: Hooks,
  request: Request,
  next: fn(Request) -> Response,
) -> Response {
  let now = system_time_ms()
  let context =
    RequestContext(
      request_id: value_or_new(header(
        request.headers,
        config.request_id_header,
      )),
      trace_id: value_or_new(
        trace_id(header(request.headers, config.trace_header)),
      ),
      tenant_id: "",
      user_id: "",
      locale: header(request.headers, "accept-language"),
      started_at_unix_ms: now,
      deadline_unix_ms: now + config.timeout_ms,
      baggage: dict.new(),
    )

  let response =
    run_http_operation(context, "middleware.pre_auth", fn() {
      case request.body_size > config.max_body_bytes {
        True ->
          problem(
            413,
            "payload_too_large",
            "request body exceeds configured limit",
          )
        False -> handle_transport(config, hooks, request, next, context)
      }
    })
    |> result_value(
      problem(500, "internal_error", "request processing failed"),
    )

  attach_headers(config, context, response)
}

fn handle_transport(
  config: Config,
  hooks: Hooks,
  request: Request,
  next: fn(Request) -> Response,
  context: RequestContext,
) -> Response {
  case config.require_https && request.scheme != "https" {
    True -> problem(426, "https_required", "HTTPS is required")
    False -> handle_policy(config, hooks, request, next, context)
  }
}
''',
)

replace_once(
    "src/gleam/src/ores_middleware.gleam",
    '''      case config.shared_auth_mode != Disabled && context.user_id == "" {
        True ->
          problem(
            401,
            "authentication_required",
            "shared-auth did not establish a user",
          )
        False -> dispatch(config, hooks, request, next, context)
      }
''',
    '''      run_http_operation(context, "middleware.authenticated", fn() {
        case config.shared_auth_mode != Disabled && context.user_id == "" {
          True ->
            problem(
              401,
              "authentication_required",
              "shared-auth did not establish a user",
            )
          False -> dispatch(config, hooks, request, next, context)
        }
      })
      |> result_value(
        problem(500, "internal_error", "request processing failed"),
      )
''',
)

replace_once(
    "src/gleam/src/ores_middleware.gleam",
    '''pub fn run_with_context(
  context: RequestContext,
  operation: fn() -> result,
) -> result {
''',
    '''fn run_http_operation(
  context: RequestContext,
  name: String,
  operation: fn() -> result,
) -> Result(result, String) {
  run_operation(context, "http", "request", name, operation)
}

pub fn run_with_context(
  context: RequestContext,
  operation: fn() -> result,
) -> result {
''',
)

replace_once(
    "src/gleam/src/ores_middleware.gleam",
    '''@external(erlang, "ores_middleware_context_ffi", "run_with_deadline")
fn run_with_deadline(
''',
    '''@external(erlang, "ores_middleware_context_ffi", "run_operation")
fn run_operation(
  context: RequestContext,
  transport: String,
  scope: String,
  name: String,
  operation: fn() -> result,
) -> Result(result, String)

@external(erlang, "ores_middleware_context_ffi", "run_with_deadline")
fn run_with_deadline(
''',
)

replace_once(
    "src/gleam/test/ores_middleware_test.gleam",
    '''pub fn production_rejects_test_only_middleware_test() {
''',
    '''fn request(request_id: String) -> ores_middleware.Request {
  ores_middleware.Request(
    method: "GET",
    path: "/profile",
    scheme: "http",
    headers: dict.from_list([#("x-request-id", request_id)]),
    body_size: 0,
    remote_ip: "198.51.100.7",
  )
}

fn lifecycle_config() -> ores_middleware.Config {
  let config = ores_middleware.default_config("lifecycle-boundary-test")
  ores_middleware.Config(
    ..config,
    require_https: False,
    rate_limit_enabled: False,
    compression_enabled: False,
    idempotency_enabled: False,
  )
}

pub fn authentication_exception_is_contained_in_base_request_context_test() {
  let hooks = ores_middleware.default_hooks()
  let hooks =
    ores_middleware.Hooks(
      ..hooks,
      authenticate: fn(_, context) {
        assert ores_middleware.current_context() == Ok(context)
        raise()
        Error("unreachable")
      },
    )
  let assert Ok(middleware) =
    ores_middleware.create_middleware(lifecycle_config(), hooks)

  let response = middleware(request("auth-panic"), fn(_) {
    panic as "handler must not run"
  })

  assert response.status == 500
  assert dict.get(response.headers, "x-request-id") == Ok("auth-panic")
  assert !string_contains(response.body, "private authentication detail")
  assert ores_middleware.current_context() == Error(Nil)
}

pub fn finalization_exception_retains_authenticated_context_test() {
  let hooks = ores_middleware.default_hooks()
  let hooks =
    ores_middleware.Hooks(
      ..hooks,
      authenticate: fn(_, _) {
        Ok(ores_middleware.AuthDecision(
          user_id: "user-42",
          tenant_id: "tenant-7",
          baggage: dict.from_list([#("otel.plan", "pro")]),
        ))
      },
      telemetry_finished: fn(context, _, _, _) {
        assert context.user_id == "user-42"
        assert context.tenant_id == "tenant-7"
        assert ores_middleware.current_context() == Ok(context)
        raise()
        Nil
      },
    )
  let assert Ok(middleware) =
    ores_middleware.create_middleware(lifecycle_config(), hooks)

  let response = middleware(request("finish-panic"), fn(_) {
    ores_middleware.Response(200, dict.new(), "ok")
  })

  assert response.status == 500
  assert dict.get(response.headers, "x-request-id") == Ok("finish-panic")
  assert !string_contains(response.body, "private finalizer detail")
  assert ores_middleware.current_context() == Error(Nil)
}

pub fn production_rejects_test_only_middleware_test() {
''',
)

replace_once(
    "src/gleam/test/ores_middleware_test.gleam",
    '''@external(erlang, "operation_boundary_test_ffi", "raise")
fn raise() -> Nil
''',
    '''@external(erlang, "operation_boundary_test_ffi", "raise")
fn raise() -> Nil

@external(erlang, "string", "find")
fn string_find(haystack: String, needle: String) -> Result(Int, Nil)

fn string_contains(haystack: String, needle: String) -> Bool {
  case string_find(haystack, needle) {
    Ok(_) -> True
    Error(_) -> False
  }
}
''',
)

import gleam/dict
import gleam/json.{type Json}
import gleam/list
import gleam/option.{None, Some}
import gleam/string
import ores_middleware as middleware
import oresoftware_next_loggers as log
import oresoftware_next_loggers_context as log_context

/// Request-specific logger passed to Gleam handlers. The context is immutable
/// and is applied explicitly even if the event is sent after the handler scope.
pub type RequestLogger {
  RequestLogger(logger: log.Logger, context: log_context.LogContext)
}

/// Middleware whose handler receives both the request and its pinned logger.
pub type Middleware =
  fn(
    middleware.Request,
    fn(middleware.Request, RequestLogger) -> middleware.Response,
  ) -> middleware.Response

/// Re-exposed ores-otel constructor surface.
pub fn options(
  app_name: String,
  runtime: String,
  id_generator: fn() -> String,
  clock: fn() -> String,
) -> log.Options {
  log.options(app_name, runtime, id_generator, clock)
}

pub fn new_logger(
  options: log.Options,
  transport: log.Transport,
) -> log.Logger {
  log.new(options, transport)
}

pub fn noop_transport() -> log.Transport {
  log.noop_transport()
}

pub fn otel_transport(
  sink: fn(log.OtelLogRecord) -> Result(Nil, String),
) -> log.Transport {
  log.otel_transport(sink)
}

pub fn supabase_transport(
  sender: fn(log.LogRecord) -> Result(Nil, String),
) -> log.Transport {
  log.supabase_transport(sender)
}

pub fn send(event: log.LogEvent) -> Result(log.LogEvent, String) {
  log.send(event)
}

/// Maps the portable middleware context to the canonical BEAM log context.
/// Only authenticated `otel.*` baggage is propagated.
pub fn to_log_context(
  context: middleware.RequestContext,
) -> log_context.LogContext {
  let fields = [
    #("request.id", json.string(context.request_id)),
    #("trace.id", json.string(context.trace_id)),
    #("request.started_at_unix_ms", json.int(context.started_at_unix_ms)),
    #("request.deadline_unix_ms", json.int(context.deadline_unix_ms)),
  ]
  let fields = case context.user_id {
    "" -> fields
    value -> list.append(fields, [#("user.id", json.string(value))])
  }
  let fields = case context.tenant_id {
    "" -> fields
    value -> list.append(fields, [#("tenant.id", json.string(value))])
  }
  let fields = case context.locale {
    "" -> fields
    value -> list.append(fields, [#("request.locale", json.string(value))])
  }
  let baggage =
    context.baggage
    |> dict.to_list
    |> list.filter(fn(entry) {
      let #(key, _) = entry
      string.starts_with(key, "otel.")
    })
    |> list.map(fn(entry) {
      let #(key, value) = entry
      #(key, json.string(value))
    })
  let user = case context.user_id {
    "" -> None
    value -> Some([#("id", json.string(value))])
  }
  let trace_id = case context.trace_id {
    "" -> None
    value -> Some(value)
  }
  let trace_ids = case context.trace_id {
    "" -> []
    value -> [value]
  }

  log_context.LogContext(
    logged_in_user: user,
    users: [],
    fields: fields,
    trace_id: trace_id,
    trace_ids: trace_ids,
    span_id: None,
    trace_flags: None,
    trace_state: None,
    baggage: baggage,
    routine_id: Some(context.request_id),
    tags: ["ores-middleware", "request"],
    context: [],
    meta: [],
  )
}

pub fn request_logger(
  logger: log.Logger,
  context: middleware.RequestContext,
) -> RequestLogger {
  RequestLogger(logger: logger, context: to_log_context(context))
}

pub fn root_logger(request_logger: RequestLogger) -> log.Logger {
  request_logger.logger
}

pub fn with_request_context(
  request_logger: RequestLogger,
  operation: fn() -> result,
) -> result {
  log_context.with_context(request_logger.context, operation)
}

pub fn trace(
  request_logger: RequestLogger,
  message: String,
  values: List(Json),
) -> log.LogEvent {
  log.trace(request_logger.logger, message, values)
  |> log_context.apply(request_logger.context)
}

pub fn debug(
  request_logger: RequestLogger,
  message: String,
  values: List(Json),
) -> log.LogEvent {
  log.debug(request_logger.logger, message, values)
  |> log_context.apply(request_logger.context)
}

pub fn info(
  request_logger: RequestLogger,
  message: String,
  values: List(Json),
) -> log.LogEvent {
  log.info(request_logger.logger, message, values)
  |> log_context.apply(request_logger.context)
}

pub fn warn(
  request_logger: RequestLogger,
  message: String,
  values: List(Json),
) -> log.LogEvent {
  log.warn(request_logger.logger, message, values)
  |> log_context.apply(request_logger.context)
}

pub fn error(
  request_logger: RequestLogger,
  message: String,
  values: List(Json),
) -> log.LogEvent {
  log.error(request_logger.logger, message, values)
  |> log_context.apply(request_logger.context)
}

pub fn fatal(
  request_logger: RequestLogger,
  message: String,
  values: List(Json),
) -> log.LogEvent {
  log.fatal(request_logger.logger, message, values)
  |> log_context.apply(request_logger.context)
}

/// Composes the standard middleware with ores-otel. Authentication remains in
/// the standard stack; the request logger is created only when the authenticated
/// context is available and file-level context loggers share the same scope.
pub fn create_middleware(
  config: middleware.Config,
  hooks: middleware.Hooks,
  logger: log.Logger,
) -> Result(Middleware, List(middleware.ValidationIssue)) {
  case middleware.create_middleware(config, hooks) {
    Error(issues) -> Error(issues)
    Ok(base) ->
      Ok(fn(request, next) {
        base(request, fn(scoped_request) {
          case middleware.current_context() {
            Error(_) -> {
              let fallback =
                RequestLogger(logger: logger, context: log_context.new())
              next(scoped_request, fallback)
            }
            Ok(context) -> {
              let request_log = request_logger(logger, context)
              let request_fields = [
                #("http.request.method", json.string(scoped_request.method)),
                #("url.path", json.string(scoped_request.path)),
              ]
              let _ =
                info(request_log, "request handler started", [])
                |> log.add_fields(request_fields)
                |> log.send

              with_request_context(request_log, fn() {
                let response = next(scoped_request, request_log)
                let response_fields =
                  list.append(request_fields, [
                    #("http.response.status_code", json.int(response.status)),
                  ])
                let _ =
                  info(request_log, "request handler completed", [])
                  |> log.add_fields(response_fields)
                  |> log.send
                response
              })
            }
          }
        })
      })
  }
}

import gleam/dict
import gleam/option.{Some}
import gleam/string
import ores_middleware
import ores_middleware/otel
import oresoftware_next_loggers as log

pub fn request_logger_is_pinned_to_authenticated_context_test() {
  let logger =
    otel.options("middleware-test", "gleam", fn() { "record-1" }, fn() {
      "2026-09-03T00:00:00Z"
    })
    |> otel.new_logger(otel.noop_transport())
  let context =
    ores_middleware.RequestContext(
      request_id: "request-42",
      trace_id: "0123456789abcdef0123456789abcdef",
      tenant_id: "tenant-7",
      user_id: "user-42",
      locale: "en-US",
      started_at_unix_ms: 1,
      deadline_unix_ms: 2,
      baggage: dict.from_list([
        #("otel.vendor", "allowed"),
        #("authorization", "must-not-propagate"),
      ]),
    )
  let request_logger = otel.request_logger(logger, context)
  let event = otel.warn(request_logger, "slow dependency", [])
  let record = log.record(event)
  let encoded = log.record_to_string(record)

  assert record.trace_id == Some("0123456789abcdef0123456789abcdef")
  assert record.routine_id == Some("request-42")
  assert string.contains(encoded, "\"request.id\":\"request-42\"")
  assert string.contains(encoded, "\"tenant.id\":\"tenant-7\"")
  assert string.contains(encoded, "\"loggedInUser\":{\"id\":\"user-42\"}")
  assert string.contains(encoded, "otel.vendor")
  assert !string.contains(encoded, "authorization")

  let _ = otel.send(event)
  let _ = log.close(logger)
}

import gleam/int
import gleam/json
import gleam/string

pub const unmatched_route_error_code = "ores.route.unmatched"

pub const unmatched_route_problem_type = "urn:ores:error:route-unmatched"

pub const unmatched_route_title = "No route matched"

pub const unmatched_route_detail = "The request target is not handled by this server."

/// The status policy for the outermost server/router fall-through boundary.
/// `NotFound` is the default for an intended origin with no matching route.
/// `MisdirectedAuthority` is only for a true origin/connection mismatch.
pub type FallthroughStatusMode {
  NotFound
  MisdirectedAuthority
}

pub type FallthroughResponse {
  FallthroughResponse(
    status: Int,
    headers: List(#(String, String)),
    body: String,
    content_length: Int,
  )
}

fn status_code(mode: FallthroughStatusMode) -> Int {
  case mode {
    NotFound -> 404
    MisdirectedAuthority -> 421
  }
}

/// Build the framework-neutral final/fall-through response value.
/// The request target itself is never accepted, so path/query/route details
/// cannot accidentally be reflected into the response. Known-route method
/// mismatches remain router-owned 405 responses.
pub fn final_fallthrough(
  method: String,
  mode: FallthroughStatusMode,
) -> FallthroughResponse {
  let status = status_code(mode)
  let encoded =
    json.object([
      #("type", json.string(unmatched_route_problem_type)),
      #("title", json.string(unmatched_route_title)),
      #("status", json.int(status)),
      #("code", json.string(unmatched_route_error_code)),
      #("detail", json.string(unmatched_route_detail)),
    ])
    |> json.to_string
  let content_length = string.byte_size(encoded)
  let headers = [
    #("cache-control", "no-store"),
    #("content-length", int.to_string(content_length)),
    #("content-type", "application/problem+json; charset=utf-8"),
    #("x-content-type-options", "nosniff"),
  ]
  let body = case string.uppercase(method) {
    "HEAD" -> ""
    _ -> encoded
  }

  FallthroughResponse(
    status: status,
    headers: headers,
    body: body,
    content_length: content_length,
  )
}

pub fn default_final_fallthrough(method: String) -> FallthroughResponse {
  final_fallthrough(method, NotFound)
}

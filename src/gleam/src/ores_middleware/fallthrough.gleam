import gleam/int
import gleam/json
import gleam/string

pub const unmatched_route_error_code = "ores.route.unmatched"

pub const unmatched_route_problem_type = "urn:ores:error:route-unmatched"

pub const unmatched_route_title = "No route matched"

pub const unmatched_route_detail = "The request target is not handled by this server."

/// The status policy for the outermost server/router fall-through boundary.
/// This is not a replacement for resource-level 404 or known-route 405 handling.
pub type FallthroughStatusMode {
  MisdirectedRequest
  NotFoundCompatibility
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
    MisdirectedRequest -> 421
    NotFoundCompatibility -> 404
  }
}

/// Build the framework-neutral final/fall-through response value.
/// The request target itself is never accepted, so path/query/route details
/// cannot accidentally be reflected into the response.
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
  final_fallthrough(method, MisdirectedRequest)
}

import gleam/dict.{type Dict}
import gleam/float
import gleam/int
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/order.{type Order, Eq, Gt, Lt}
import gleam/string

pub const contract_id = "ores.docs-serving/v1"

pub const docs_format_header = "X-Ores-Docs-Format"

pub const contract_digest_header = "X-Ores-Contract-SHA256"

pub type Representation {
  Html
  Catalog
  OpenApi
  OpenRpc
  Connect
  HyperSchema
}

pub type Action {
  Pass
  Serve
  MethodNotAllowed
  NotAcceptable
  StoppedForEvaluation
}

pub type Request {
  Request(
    method: String,
    path: String,
    accept: String,
    format: String,
    runtime_contract_digest: String,
    docs_contract_digest: String,
  )
}

pub type Decision {
  Decision(
    action: Action,
    status: Option(Int),
    representation: Option(Representation),
    head_only: Bool,
    headers: Dict(String, String),
  )
}

type PathKind {
  UnknownPath
  GenericPath
  FixedPath(Representation)
}

type FormatSelection {
  FormatAbsent
  FormatKnown(Representation)
  FormatInvalid
}

type MediaRange {
  MediaRange(media: String, quality: Float, index: Int)
}

/// Evaluate a routing-neutral docs request without loading a response body or
/// registering framework routes. Unknown paths pass through without headers.
pub fn decide(request: Request) -> Decision {
  let path = path_without_query(request.path)
  case path_kind(path) {
    UnknownPath -> pass()
    kind -> decide_known_path(request, kind)
  }
}

pub fn action_name(action: Action) -> String {
  case action {
    Pass -> "pass"
    Serve -> "serve"
    MethodNotAllowed -> "method-not-allowed"
    NotAcceptable -> "not-acceptable"
    StoppedForEvaluation -> "stopped-for-evaluation"
  }
}

pub fn representation_name(representation: Representation) -> String {
  case representation {
    Html -> "html"
    Catalog -> "catalog"
    OpenApi -> "openapi"
    OpenRpc -> "openrpc"
    Connect -> "connect"
    HyperSchema -> "hyper-schema"
  }
}

fn decide_known_path(request: Request, kind: PathKind) -> Decision {
  let method = request.method |> string.trim |> string.uppercase
  case method == "GET" || method == "HEAD" {
    False -> reject(MethodNotAllowed, 405, [#("Allow", "GET, HEAD")])
    True ->
      case
        digest_failure(
          request.runtime_contract_digest,
          request.docs_contract_digest,
        )
      {
        True -> reject(StoppedForEvaluation, 503, [])
        False -> decide_representation(request, kind, method == "HEAD")
      }
  }
}

fn decide_representation(
  request: Request,
  kind: PathKind,
  head_only: Bool,
) -> Decision {
  case normalized_format(request.format) {
    FormatInvalid -> reject(NotAcceptable, 406, [])
    format ->
      case select_representation(kind, request.accept, format) {
        Error(_) -> reject(NotAcceptable, 406, [])
        Ok(representation) ->
          case accepts_representation(request.accept, representation) {
            False -> reject(NotAcceptable, 406, [])
            True ->
              Decision(
                action: Serve,
                status: Some(200),
                representation: Some(representation),
                head_only: head_only,
                headers: headers_for_representation(
                  representation,
                  string.trim(request.docs_contract_digest),
                ),
              )
          }
      }
  }
}

fn pass() -> Decision {
  Decision(
    action: Pass,
    status: None,
    representation: None,
    head_only: False,
    headers: dict.new(),
  )
}

fn reject(
  action: Action,
  status: Int,
  extra_headers: List(#(String, String)),
) -> Decision {
  let headers =
    list.fold(
      extra_headers,
      base_headers("application/json; charset=utf-8"),
      fn(headers, header) {
        let #(name, value) = header
        dict.insert(headers, name, value)
      },
    )
  Decision(
    action: action,
    status: Some(status),
    representation: None,
    head_only: False,
    headers: headers,
  )
}

fn headers_for_representation(
  representation: Representation,
  digest: String,
) -> Dict(String, String) {
  let headers = base_headers(content_type(representation))
  let headers = case representation {
    Html ->
      headers
      |> dict.insert("X-Frame-Options", "DENY")
      |> dict.insert(
        "Content-Security-Policy",
        "default-src 'none'; style-src 'unsafe-inline'; img-src 'none'; frame-ancestors 'none'; base-uri 'none'; object-src 'none'; form-action 'none'; connect-src 'none'; script-src 'none'",
      )
    _ -> headers
  }
  case digest == "" {
    True -> headers
    False -> dict.insert(headers, contract_digest_header, digest)
  }
}

fn base_headers(content_type: String) -> Dict(String, String) {
  dict.from_list([
    #("Cache-Control", "no-store"),
    #("Pragma", "no-cache"),
    #("X-Content-Type-Options", "nosniff"),
    #("Referrer-Policy", "no-referrer"),
    #("Permissions-Policy", "camera=(), microphone=(), geolocation=()"),
    #("Vary", "Accept, " <> docs_format_header),
    #("Content-Type", content_type),
  ])
}

fn content_type(representation: Representation) -> String {
  case representation {
    Html -> "text/html; charset=utf-8"
    Catalog -> "application/vnd.ores.api-docs+json; charset=utf-8"
    OpenApi -> "application/vnd.oai.openapi+json; charset=utf-8"
    OpenRpc -> "application/openrpc+json; charset=utf-8"
    Connect -> "application/vnd.ores.connect+json; charset=utf-8"
    HyperSchema -> "application/schema+json; charset=utf-8"
  }
}

fn path_without_query(path: String) -> String {
  case string.split(path, "?") {
    [path, ..] -> path
    [] -> path
  }
}

fn path_kind(path: String) -> PathKind {
  case path {
    "/docs/api" | "/api/docs" | "/api-docs" -> GenericPath
    "/api/docs.json" | "/api-docs.json" -> FixedPath(Catalog)
    "/openapi.json" -> FixedPath(OpenApi)
    "/openrpc.json" -> FixedPath(OpenRpc)
    "/connect.json" -> FixedPath(Connect)
    "/hyper-schema.json" -> FixedPath(HyperSchema)
    _ -> UnknownPath
  }
}

fn normalized_format(value: String) -> FormatSelection {
  let value = value |> string.trim |> string.lowercase
  case value {
    "" -> FormatAbsent
    "html" -> FormatKnown(Html)
    "catalog" -> FormatKnown(Catalog)
    "openapi" -> FormatKnown(OpenApi)
    "openrpc" -> FormatKnown(OpenRpc)
    "connect" -> FormatKnown(Connect)
    "hyper-schema" -> FormatKnown(HyperSchema)
    _ -> FormatInvalid
  }
}

fn select_representation(
  kind: PathKind,
  accept: String,
  format: FormatSelection,
) -> Result(Representation, Nil) {
  case kind, format {
    GenericPath, FormatAbsent -> negotiate_generic(accept)
    GenericPath, FormatKnown(representation) -> Ok(representation)
    FixedPath(representation), FormatAbsent -> Ok(representation)
    FixedPath(representation), FormatKnown(requested) ->
      case representation == requested {
        True -> Ok(representation)
        False -> Error(Nil)
      }
    _, FormatInvalid -> Error(Nil)
    UnknownPath, _ -> Error(Nil)
  }
}

fn negotiate_generic(accept: String) -> Result(Representation, Nil) {
  let ranges = parse_accept(accept)
  case ranges {
    [] ->
      case string.trim(accept) == "" {
        True -> Ok(Html)
        False -> Error(Nil)
      }
    _ -> list.find_map(ranges, fn(item) { media_representation(item.media) })
  }
}

fn accepts_representation(
  accept: String,
  representation: Representation,
) -> Bool {
  case string.trim(accept) == "" {
    True -> True
    False ->
      list.any(parse_accept(accept), fn(item) {
        case item.media {
          "*/*" -> True
          "application/*" | "application/json" -> representation != Html
          media ->
            case media_representation(media) {
              Ok(candidate) -> candidate == representation
              Error(_) -> False
            }
        }
      })
  }
}

fn parse_accept(value: String) -> List(MediaRange) {
  value
  |> string.split(",")
  |> list.index_map(fn(raw_part, index) { parse_media_range(raw_part, index) })
  |> list.filter_map(fn(result) { result })
  |> list.sort(by: compare_media_ranges)
}

fn parse_media_range(raw_part: String, index: Int) -> Result(MediaRange, Nil) {
  case string.split(raw_part, ";") {
    [] -> Error(Nil)
    [raw_media, ..parameters] -> {
      let media = raw_media |> string.trim |> string.lowercase
      case media == "" {
        True -> Error(Nil)
        False ->
          case parse_quality(parameters, 1.0) {
            Ok(quality) ->
              case quality >. 0.0 {
                True ->
                  Ok(MediaRange(media: media, quality: quality, index: index))
                False -> Error(Nil)
              }
            Error(_) -> Error(Nil)
          }
      }
    }
  }
}

fn parse_quality(
  parameters: List(String),
  current: Float,
) -> Result(Float, Nil) {
  case parameters {
    [] -> Ok(current)
    [raw_parameter, ..rest] ->
      case string.split(raw_parameter, "=") {
        [raw_name, raw_value] -> {
          let name = raw_name |> string.trim |> string.lowercase
          case name {
            "q" ->
              case float.parse(string.trim(raw_value)) {
                Ok(value) ->
                  case value >=. 0.0 && value <=. 1.0 {
                    True -> parse_quality(rest, value)
                    False -> Error(Nil)
                  }
                Error(_) -> Error(Nil)
              }
            _ -> parse_quality(rest, current)
          }
        }
        [raw_name, ..] -> {
          let name = raw_name |> string.trim |> string.lowercase
          case name {
            "q" -> Error(Nil)
            _ -> parse_quality(rest, current)
          }
        }
        [] -> parse_quality(rest, current)
      }
  }
}

fn compare_media_ranges(left: MediaRange, right: MediaRange) -> Order {
  case float.compare(left.quality, right.quality) {
    Gt -> Lt
    Lt -> Gt
    Eq -> int.compare(left.index, right.index)
  }
}

fn media_representation(media: String) -> Result(Representation, Nil) {
  case media {
    "*/*" -> Ok(Html)
    "application/*" -> Ok(Catalog)
    "text/html" -> Ok(Html)
    "application/vnd.ores.api-docs+json" | "application/json" -> Ok(Catalog)
    "application/vnd.oai.openapi+json" | "application/openapi+json" ->
      Ok(OpenApi)
    "application/openrpc+json" -> Ok(OpenRpc)
    "application/vnd.ores.connect+json" -> Ok(Connect)
    "application/schema+json" -> Ok(HyperSchema)
    _ -> Error(Nil)
  }
}

fn digest_failure(runtime_digest: String, docs_digest: String) -> Bool {
  let runtime_digest = string.trim(runtime_digest)
  let docs_digest = string.trim(docs_digest)
  let runtime_present = runtime_digest != ""
  let docs_present = docs_digest != ""
  case runtime_present && !valid_sha256_digest(runtime_digest) {
    True -> True
    False ->
      case docs_present && !valid_sha256_digest(docs_digest) {
        True -> True
        False ->
          runtime_present && { !docs_present || runtime_digest != docs_digest }
      }
  }
}

fn valid_sha256_digest(value: String) -> Bool {
  string.length(value) == 64
  && list.all(string.to_graphemes(value), fn(character) {
    string.contains("0123456789abcdef", character)
  })
}

import gleam/dict
import gleam/dynamic.{type Dynamic}
import gleam/dynamic/decode
import gleam/io
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import idempotency_record

const witness_schema = "ores.generated-runtime-witness/v1"

const int32_minimum = -2_147_483_648

const int32_maximum = 2_147_483_647

@external(erlang, "runtime_ffi", "get_env")
fn get_env(name: String) -> Result(String, Nil)

@external(erlang, "runtime_ffi", "read_text")
fn read_text(path: String) -> Result(String, Nil)

@external(erlang, "runtime_ffi", "valid_rfc3339")
fn valid_rfc3339(value: String) -> Bool

@external(erlang, "runtime_ffi", "source_contains_all")
fn source_contains_all(path: String, fragments: List(String)) -> Bool

type Fixture {
  Fixture(
    model: String,
    wire_fields: List(String),
    required_fields: List(String),
    optional_fields: List(String),
    statuses: List(String),
    cases: List(TestCase),
  )
}

type TestCase {
  TestCase(id: String, expected: String, value: Dynamic)
}

type CaseResult {
  CaseResult(id: String, accepted: Bool, normalized: Option(json.Json))
}

fn test_case_decoder() -> decode.Decoder(TestCase) {
  use id <- decode.field("id", decode.string)
  use expected <- decode.field("expect", decode.string)
  use value <- decode.field("value", decode.dynamic)
  decode.success(TestCase(id:, expected:, value:))
}

fn fixture_decoder() -> decode.Decoder(Fixture) {
  use model <- decode.field("model", decode.string)
  use wire_fields <- decode.field("wireFields", decode.list(decode.string))
  use required_fields <- decode.field(
    "requiredFields",
    decode.list(decode.string),
  )
  use optional_fields <- decode.field(
    "optionalFields",
    decode.list(decode.string),
  )
  use statuses <- decode.field("statuses", decode.list(decode.string))
  use cases <- decode.field("cases", decode.list(test_case_decoder()))
  decode.success(Fixture(
    model:,
    wire_fields:,
    required_fields:,
    optional_fields:,
    statuses:,
    cases:,
  ))
}

fn int32_decoder() -> decode.Decoder(Int) {
  use value <- decode.then(decode.int)
  case value >= int32_minimum && value <= int32_maximum {
    True -> decode.success(value)
    False -> decode.failure(0, expected: "Int32")
  }
}

fn date_time_decoder() -> decode.Decoder(String) {
  use value <- decode.then(decode.string)
  case valid_rfc3339(value) {
    True -> decode.success(value)
    False -> decode.failure("", expected: "RFC3339DateTime")
  }
}

fn status_decoder() -> decode.Decoder(idempotency_record.IdempotencyStatus) {
  use value <- decode.then(decode.string)
  case idempotency_record.idempotency_status_from_string(value) {
    Ok(status) -> decode.success(status)
    Error(_) ->
      decode.failure(idempotency_record.Pending, expected: "IdempotencyStatus")
  }
}

fn default_record() -> idempotency_record.IdempotencyRecord {
  idempotency_record.IdempotencyRecord(
    created_at: "",
    expires_at: "",
    id: "",
    idempotency_key: "",
    request_hash: "",
    response_body: None,
    response_status: None,
    status: idempotency_record.Pending,
    tenant_id: "",
  )
}

fn record_decoder(
  allowed_fields: List(String),
) -> decode.Decoder(idempotency_record.IdempotencyRecord) {
  use object <- decode.then(decode.dict(decode.string, decode.dynamic))
  case
    list.all(dict.keys(object), fn(key) { list.contains(allowed_fields, key) })
  {
    False -> decode.failure(default_record(), expected: "IdempotencyRecord")
    True -> {
      use created_at <- decode.field("createdAt", date_time_decoder())
      use expires_at <- decode.field("expiresAt", date_time_decoder())
      use id <- decode.field("id", decode.string)
      use idempotency_key <- decode.field("idempotencyKey", decode.string)
      use request_hash <- decode.field("requestHash", decode.string)
      use response_body <- decode.optional_field(
        "responseBody",
        None,
        decode.map(decode.string, fn(value) { Some(value) }),
      )
      use response_status <- decode.optional_field(
        "responseStatus",
        None,
        decode.map(int32_decoder(), fn(value) { Some(value) }),
      )
      use status <- decode.field("status", status_decoder())
      use tenant_id <- decode.field("tenantId", decode.string)
      decode.success(idempotency_record.IdempotencyRecord(
        created_at:,
        expires_at:,
        id:,
        idempotency_key:,
        request_hash:,
        response_body:,
        response_status:,
        status:,
        tenant_id:,
      ))
    }
  }
}

fn status_to_string(status: idempotency_record.IdempotencyStatus) -> String {
  case status {
    idempotency_record.Pending -> "pending"
    idempotency_record.Succeeded -> "succeeded"
    idempotency_record.Failed -> "failed"
  }
}

fn normalize(record: idempotency_record.IdempotencyRecord) -> json.Json {
  let idempotency_record.IdempotencyRecord(
    created_at:,
    expires_at:,
    id:,
    idempotency_key:,
    request_hash:,
    response_body:,
    response_status:,
    status:,
    tenant_id:,
  ) = record
  let entries = [
    #("createdAt", json.string(created_at)),
    #("expiresAt", json.string(expires_at)),
    #("id", json.string(id)),
    #("idempotencyKey", json.string(idempotency_key)),
    #("requestHash", json.string(request_hash)),
    #("status", json.string(status_to_string(status))),
    #("tenantId", json.string(tenant_id)),
  ]
  let entries = case response_body {
    Some(value) -> list.append(entries, [#("responseBody", json.string(value))])
    None -> entries
  }
  let entries = case response_status {
    Some(value) -> list.append(entries, [#("responseStatus", json.int(value))])
    None -> entries
  }
  json.object(entries)
}

fn run_case(test_case: TestCase, fixture: Fixture) -> CaseResult {
  case decode.run(test_case.value, record_decoder(fixture.wire_fields)) {
    Ok(record) ->
      CaseResult(
        id: test_case.id,
        accepted: True,
        normalized: Some(normalize(record)),
      )
    Error(_) -> CaseResult(id: test_case.id, accepted: False, normalized: None)
  }
}

fn encode_case_result(result: CaseResult) -> json.Json {
  json.object([
    #("id", json.string(result.id)),
    #("accepted", json.bool(result.accepted)),
    #("normalized", case result.normalized {
      Some(value) -> value
      None -> json.null()
    }),
  ])
}

fn source_shape_fragments() -> List(String) {
  [
    "created_at: String",
    "expires_at: String",
    "id: String",
    "idempotency_key: String",
    "request_hash: String",
    "response_body: Option(String)",
    "response_status: Option(Int)",
    "status: IdempotencyStatus",
    "tenant_id: String",
    "Pending",
    "Succeeded",
    "Failed",
  ]
}

pub fn main() {
  let assert Ok(fixture_path) = get_env("ORES_RUNTIME_FIXTURE")
  let assert Ok(authority) = get_env("ORES_RUNTIME_AUTHORITY")
  let assert Ok(generated_path) = get_env("ORES_RUNTIME_GENERATED_SOURCE")
  let assert True =
    source_contains_all(generated_path, source_shape_fragments())
  let assert Ok(fixture_text) = read_text(fixture_path)
  let assert Ok(fixture) = json.parse(fixture_text, fixture_decoder())

  let cases =
    list.map(fixture.cases, fn(test_case) { run_case(test_case, fixture) })
  let status_acceptance =
    list.append(fixture.statuses, ["__unknown__"])
    |> list.map(fn(status) {
      #(
        status,
        idempotency_record.idempotency_status_from_string(status)
          |> result.is_ok
          |> json.bool,
      )
    })
    |> json.object

  json.object([
    #("schema", json.string(witness_schema)),
    #("authority", json.string(authority)),
    #("language", json.string("gleam")),
    #("model", json.string(fixture.model)),
    #("wireFields", json.array(fixture.wire_fields, of: json.string)),
    #("requiredFields", json.array(fixture.required_fields, of: json.string)),
    #("optionalFields", json.array(fixture.optional_fields, of: json.string)),
    #("statuses", json.array(fixture.statuses, of: json.string)),
    #("statusAcceptance", status_acceptance),
    #("cases", json.array(cases, of: encode_case_result)),
  ])
  |> json.to_string
  |> io.println
}

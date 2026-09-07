import gleam/dict
import gleam/int
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/string
import gleeunit
import ores_middleware/docs_serving

pub fn main() {
  gleeunit.main()
}

pub fn shared_conformance_fixture_test() {
  let rows = fixture_rows()
  assert list.length(rows) > 0

  list.each(rows, fn(row) {
    let #(
      name,
      method,
      path,
      accept,
      format,
      runtime_digest,
      docs_digest,
      expected_action,
      expected_status,
      expected_representation,
      expected_head_only,
    ) = row

    assert name != ""

    let request =
      docs_serving.Request(
        method: method,
        path: path,
        accept: optional(accept),
        format: optional(format),
        runtime_contract_digest: optional(runtime_digest),
        docs_contract_digest: optional(docs_digest),
      )
    let decision = docs_serving.decide(request)

    assert docs_serving.action_name(decision.action) == expected_action
    assert status_name(decision.status) == expected_status
    assert representation_name(decision.representation)
      == expected_representation
    assert decision.head_only == { expected_head_only == "true" }

    case decision.action {
      docs_serving.Pass -> {
        assert dict.size(decision.headers) == 0
        Nil
      }
      _ -> {
        assert dict.get(decision.headers, "Cache-Control") == Ok("no-store")
        let assert Ok(vary) = dict.get(decision.headers, "Vary")
        assert string.contains(vary, docs_serving.docs_format_header)
        Nil
      }
    }

    case decision.action {
      docs_serving.MethodNotAllowed -> {
        assert dict.get(decision.headers, "Allow") == Ok("GET, HEAD")
        Nil
      }
      _ -> Nil
    }

    case decision.representation {
      Some(docs_serving.Html) -> {
        assert dict.get(decision.headers, "X-Frame-Options") == Ok("DENY")
        let assert Ok(policy) =
          dict.get(decision.headers, "Content-Security-Policy")
        assert string.contains(policy, "frame-ancestors 'none'")
        Nil
      }
      _ -> Nil
    }

    case
      decision.action == docs_serving.Serve
      && string.length(optional(docs_digest)) == 64
    {
      True -> {
        assert dict.get(decision.headers, docs_serving.contract_digest_header)
          == Ok(docs_digest)
        Nil
      }
      False -> Nil
    }
  })
}

fn optional(value: String) -> String {
  case value {
    "-" -> ""
    value -> value
  }
}

fn status_name(status: Option(Int)) -> String {
  case status {
    None -> "-"
    Some(status) -> int.to_string(status)
  }
}

fn representation_name(
  representation: Option(docs_serving.Representation),
) -> String {
  case representation {
    None -> "-"
    Some(representation) -> docs_serving.representation_name(representation)
  }
}

@external(erlang, "docs_serving_test_ffi", "fixture_rows")
fn fixture_rows() -> List(
  #(
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
  ),
)

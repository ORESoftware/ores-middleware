use std::fs;

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

fn write(path: &str, content: &str) {
    fs::write(path, content).unwrap_or_else(|error| panic!("failed to write {path}: {error}"));
}

fn replace_exact(path: &str, old: &str, new: &str) {
    let content = read(path);
    let matches = content.match_indices(old).count();
    assert_eq!(matches, 1, "expected one match in {path}, found {matches}");
    write(path, &content.replacen(old, new, 1));
}

fn main() {
    replace_exact(
        "src/ts/src/index.ts",
        r#"function parseTraceId(value: string | null): string | undefined {
  const part = value?.split("-")[1]?.toLowerCase();
  return part &&
    /^[0-9a-f]{32}$/.test(part) &&
    part !== "00000000000000000000000000000000"
    ? part
    : undefined;
}"#,
        r#"function parseTraceId(value: string | null): string | undefined {
  return validTraceparent(value)?.split("-")[1];
}"#,
    );

    replace_exact(
        "src/golang/middleware.go",
        r#"func parseTraceID(value string) string {
	parts := strings.Split(value, "-")
	if len(parts) < 2 || len(parts[1]) != 32 {
		return ""
	}
	traceID := strings.ToLower(parts[1])
	if traceID == strings.Repeat("0", 32) {
		return ""
	}
	if _, err := hex.DecodeString(traceID); err != nil {
		return ""
	}
	return traceID
}"#,
        r#"func parseTraceID(value string) string {
	normalized := validTraceparent(value)
	if normalized == "" {
		return ""
	}
	return strings.Split(normalized, "-")[1]
}"#,
    );

    replace_exact(
        "src/rust/src/pipeline.rs",
        r#"fn parse_trace_id(header: Option<&String>) -> Option<String> {
    let value = header?;
    let mut parts = value.split('-');
    let _version = parts.next()?;
    let trace_id = parts.next()?.to_ascii_lowercase();
    (trace_id.len() == 32
        && trace_id != "00000000000000000000000000000000"
        && trace_id.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then_some(trace_id)
}"#,
        r#"fn parse_trace_id(header: Option<&String>) -> Option<String> {
    let mut parts = header?.split('-');
    let version = parts.next()?.to_ascii_lowercase();
    let trace_id = parts.next()?.to_ascii_lowercase();
    let parent_id = parts.next()?.to_ascii_lowercase();
    let flags = parts.next()?.to_ascii_lowercase();
    if parts.next().is_some() {
        return None;
    }

    (version == "00"
        && trace_id.len() == 32
        && trace_id != "00000000000000000000000000000000"
        && trace_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        && parent_id.len() == 16
        && parent_id != "0000000000000000"
        && parent_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        && flags.len() == 2
        && flags.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then_some(trace_id)
}"#,
    );

    replace_exact(
        "src/gleam/src/ores_middleware.gleam",
        r#"fn trace_id(value: String) -> String {
  case string.split(value, "-") {
    [_, trace, ..] ->
      case valid_hex_identifier(trace, 32, "00000000000000000000000000000000") {
        True -> string.lowercase(trace)
        False -> ""
      }
    _ -> ""
  }
}"#,
        r#"fn trace_id(value: String) -> String {
  case normalize_traceparent(value) {
    Ok(value) ->
      case string.split(value, "-") {
        [_, trace, _, _] -> trace
        _ -> ""
      }
    Error(_) -> ""
  }
}"#,
    );

    replace_exact(
        "src/elixir/lib/ores_middleware/plug.ex",
        r#"  defp parse_trace_id(value) when is_binary(value) do
    case String.split(value, "-") do
      [_, trace | _] ->
        trace = String.downcase(trace)
        if valid_hex_id?(trace, 32, @zero_trace_id), do: trace
      _ -> nil
    end
  end"#,
        r#"  defp parse_trace_id(value) when is_binary(value) do
    case normalize_traceparent(value) do
      nil -> nil
      normalized -> normalized |> String.split("-") |> Enum.at(1)
    end
  end"#,
    );

    replace_exact(
        "src/erlang/src/ores_middleware.erl",
        r#"trace_or_new(Value) when is_binary(Value) ->
    case binary:split(Value, <<"-">>, [global]) of
        [_, Trace | _] ->
            Lower = string:lowercase(Trace),
            case valid_hex_id(Lower, 32, <<"00000000000000000000000000000000">>) of
                true -> Lower;
                false -> new_id()
            end;
        _ -> new_id()
    end;
trace_or_new(_) -> new_id()."#,
        r#"trace_or_new(Value) when is_binary(Value) ->
    case normalize_traceparent(Value) of
        {ok, Normalized} ->
            [_, Trace, _, _] = binary:split(Normalized, <<"-">>, [global]),
            Trace;
        error -> new_id()
    end;
trace_or_new(_) -> new_id()."#,
    );

    replace_exact(
        "docs/RESPONSE_TRACE_CONTEXT.md",
        "- Inbound `traceparent` is parsed only to continue or replace the trace ID.\n",
        "- Inbound `traceparent` is accepted only as a complete valid version-00 carrier.\n  A malformed carrier, unsupported version, invalid flags, all-zero trace ID, or\n  all-zero parent span ID starts a new trace rather than retaining a partial ID.\n",
    );

    write(
        "scripts/check_traceparent_policy.rs",
        r#"use std::fs;

const PRODUCTION_SOURCES: &[&str] = &[
    "src/ts/src/index.ts",
    "src/ts/src/adapters.ts",
    "src/golang/middleware.go",
    "src/rust/src/pipeline.rs",
    "src/gleam/src/ores_middleware.gleam",
    "src/elixir/lib/ores_middleware/plug.ex",
    "src/erlang/src/ores_middleware.erl",
    "src/erlang/src/ores_middleware_cowboy.erl",
];

fn main() {
    let mut failures = Vec::new();
    for path in PRODUCTION_SOURCES {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
        for forbidden in [
            "-0000000000000000-01",
            "inboundSpanId",
            "validSpanId(context.spanId) ??",
        ] {
            if content.contains(forbidden) {
                failures.push(format!(
                    "{path}: forbidden response trace-context pattern {forbidden:?}"
                ));
            }
        }
    }

    for (path, required) in [
        ("src/ts/src/index.ts", "function validTraceparent"),
        (
            "src/ts/src/index.ts",
            "return validTraceparent(value)?.split(\"-\")[1];",
        ),
        ("src/golang/middleware.go", "func validTraceparent"),
        (
            "src/golang/middleware.go",
            "normalized := validTraceparent(value)",
        ),
        ("src/rust/src/pipeline.rs", "let parent_id = parts.next()?"),
        (
            "src/rust/src/pipeline.rs",
            "parent_id != \"0000000000000000\"",
        ),
        (
            "src/gleam/src/ores_middleware.gleam",
            "fn normalize_traceparent",
        ),
        (
            "src/gleam/src/ores_middleware.gleam",
            "case normalize_traceparent(value)",
        ),
        (
            "src/elixir/lib/ores_middleware/plug.ex",
            "defp normalize_traceparent",
        ),
        (
            "src/elixir/lib/ores_middleware/plug.ex",
            "case normalize_traceparent(value) do",
        ),
        (
            "src/erlang/src/ores_middleware.erl",
            "normalize_traceparent(Value)",
        ),
        (
            "src/erlang/src/ores_middleware.erl",
            "case normalize_traceparent(Value) of",
        ),
    ] {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
        if !content.contains(required) {
            failures.push(format!(
                "{path}: missing required safety marker {required:?}"
            ));
        }
    }

    if !failures.is_empty() {
        eprintln!("trace-context safety audit failed:");
        for failure in failures {
            eprintln!("- {failure}");
        }
        std::process::exit(1);
    }

    println!("trace-context safety audit passed");
}
"#,
    );

    write(
        "src/ts/test/traceparent-policy.test.mjs",
        r#"import test from "node:test";
import assert from "node:assert/strict";

import {
  createMiddleware,
  currentContext,
  defaultConfig
} from "../dist/index.js";

const ZERO_TRACE_ID = "00000000000000000000000000000000";
const VALID_TRACE_ID = "0123456789abcdef0123456789abcdef";
const VALID_PARENT_SPAN_ID = "0123456789abcdef";
const VALID_SERVER_SPAN_ID = "fedcba9876543210";
const VALID_RESPONSE_TRACEPARENT =
  `00-${VALID_TRACE_ID}-${VALID_SERVER_SPAN_ID}-01`;

function testConfig() {
  const config = defaultConfig("traceparent-policy-test");
  config.environment = "test";
  config.settings.tls.mode = "disabled";
  config.settings.tls.requireHttps = false;
  config.settings.rateLimit.enabled = false;
  config.settings.idempotency.enabled = false;
  config.settings.compression.enabled = false;
  return config;
}

function requestWithTraceparent(traceparent) {
  return new Request("http://example.test/trace", {
    headers: { accept: "application/json", traceparent }
  });
}

function request(traceId = VALID_TRACE_ID) {
  return requestWithTraceparent(
    `00-${traceId}-${VALID_PARENT_SPAN_ID}-01`
  );
}

test("portable middleware does not echo the inbound parent as a response span", async () => {
  const middleware = createMiddleware(testConfig());
  const response = await middleware(request(), async () =>
    new Response(null, { status: 204 })
  );

  assert.equal(response.headers.get("traceparent"), null);
  assert.ok(response.headers.get("x-request-id"));
});

test("only a valid tracer-owned response traceparent is preserved", async () => {
  const middleware = createMiddleware(testConfig());
  const cases = [
    [VALID_RESPONSE_TRACEPARENT.toUpperCase(), VALID_RESPONSE_TRACEPARENT],
    [`00-${VALID_TRACE_ID}-0000000000000000-01`, null],
    [`00-${ZERO_TRACE_ID}-${VALID_SERVER_SPAN_ID}-01`, null],
    ["00-not-hex-not-a-span-01", null]
  ];

  for (const [candidate, expected] of cases) {
    const response = await middleware(request(), async () =>
      new Response(null, {
        status: 204,
        headers: { traceparent: candidate }
      })
    );
    assert.equal(response.headers.get("traceparent"), expected);
  }
});

test("invalid inbound traceparent restarts the entire trace", async () => {
  const middleware = createMiddleware(testConfig());
  const cases = [
    ["zero trace", `00-${ZERO_TRACE_ID}-${VALID_PARENT_SPAN_ID}-01`, ZERO_TRACE_ID],
    ["zero parent", `00-${VALID_TRACE_ID}-0000000000000000-01`, VALID_TRACE_ID],
    ["non-hex parent", `00-${VALID_TRACE_ID}-zzzzzzzzzzzzzzzz-01`, VALID_TRACE_ID],
    ["unsupported version", `ff-${VALID_TRACE_ID}-${VALID_PARENT_SPAN_ID}-01`, VALID_TRACE_ID],
    ["non-hex flags", `00-${VALID_TRACE_ID}-${VALID_PARENT_SPAN_ID}-zz`, VALID_TRACE_ID],
    ["extra field", `00-${VALID_TRACE_ID}-${VALID_PARENT_SPAN_ID}-01-extra`, VALID_TRACE_ID]
  ];

  for (const [name, candidate, forbiddenTraceId] of cases) {
    let observedTraceId;
    const response = await middleware(
      requestWithTraceparent(candidate),
      async () => {
        observedTraceId = currentContext()?.traceId;
        return new Response(null, { status: 204 });
      }
    );

    assert.match(observedTraceId, /^[0-9a-f]{32}$/, name);
    assert.notEqual(observedTraceId, forbiddenTraceId, name);
    assert.equal(response.headers.get("traceparent"), null, name);
  }
});
"#,
    );

    write(
        "src/golang/traceparent_policy_test.go",
        r#"package oresmiddleware

import (
	"encoding/hex"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

const (
	zeroTraceID       = "00000000000000000000000000000000"
	validTraceID      = "0123456789abcdef0123456789abcdef"
	validParentSpanID = "0123456789abcdef"
	validServerSpanID = "fedcba9876543210"
)

func requestWithTraceparent(value string) *http.Request {
	request := httptest.NewRequest(http.MethodGet, "http://example.test/trace", nil)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("Traceparent", value)
	return request
}

func traceparentRequest(traceID string) *http.Request {
	return requestWithTraceparent("00-" + traceID + "-" + validParentSpanID + "-01")
}

func TestInboundParentIsNotRelabelledAsResponseSpan(t *testing.T) {
	stack, err := New(testConfig(), Dependencies{})
	if err != nil {
		t.Fatal(err)
	}
	handler := stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		writer.WriteHeader(http.StatusNoContent)
	}))
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, traceparentRequest(validTraceID))

	if got := response.Header().Get("Traceparent"); got != "" {
		t.Fatalf("unexpected synthesized response traceparent %q", got)
	}
}

func TestOnlyValidTracerOwnedResponseTraceparentIsPreserved(t *testing.T) {
	valid := "00-" + validTraceID + "-" + validServerSpanID + "-01"
	tests := []struct {
		name      string
		candidate string
		expected  string
	}{
		{name: "valid", candidate: strings.ToUpper(valid), expected: valid},
		{name: "zero span", candidate: "00-" + validTraceID + "-0000000000000000-01"},
		{name: "zero trace", candidate: "00-" + zeroTraceID + "-" + validServerSpanID + "-01"},
		{name: "malformed", candidate: "00-not-hex-not-a-span-01"},
	}

	for _, item := range tests {
		t.Run(item.name, func(t *testing.T) {
			stack, err := New(testConfig(), Dependencies{})
			if err != nil {
				t.Fatal(err)
			}
			handler := stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
				writer.Header().Set("Traceparent", item.candidate)
				writer.WriteHeader(http.StatusNoContent)
			}))
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, traceparentRequest(validTraceID))
			if got := response.Header().Get("Traceparent"); got != item.expected {
				t.Fatalf("traceparent=%q expected=%q", got, item.expected)
			}
		})
	}
}

func TestInvalidInboundTraceparentRestartsEntireTrace(t *testing.T) {
	tests := []struct {
		name      string
		candidate string
		forbidden string
	}{
		{name: "zero trace", candidate: "00-" + zeroTraceID + "-" + validParentSpanID + "-01", forbidden: zeroTraceID},
		{name: "zero parent", candidate: "00-" + validTraceID + "-0000000000000000-01", forbidden: validTraceID},
		{name: "non-hex parent", candidate: "00-" + validTraceID + "-zzzzzzzzzzzzzzzz-01", forbidden: validTraceID},
		{name: "unsupported version", candidate: "ff-" + validTraceID + "-" + validParentSpanID + "-01", forbidden: validTraceID},
		{name: "non-hex flags", candidate: "00-" + validTraceID + "-" + validParentSpanID + "-zz", forbidden: validTraceID},
		{name: "extra field", candidate: "00-" + validTraceID + "-" + validParentSpanID + "-01-extra", forbidden: validTraceID},
	}

	for _, item := range tests {
		t.Run(item.name, func(t *testing.T) {
			stack, err := New(testConfig(), Dependencies{})
			if err != nil {
				t.Fatal(err)
			}
			observed := make(chan string, 1)
			handler := stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				context, ok := CurrentContext(request.Context())
				if !ok {
					observed <- ""
				} else {
					observed <- context.TraceID
				}
				writer.WriteHeader(http.StatusNoContent)
			}))
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, requestWithTraceparent(item.candidate))

			traceID := <-observed
			if len(traceID) != 32 {
				t.Fatalf("replacement trace ID has length %d: %q", len(traceID), traceID)
			}
			if _, err := hex.DecodeString(traceID); err != nil {
				t.Fatalf("replacement trace ID is not hexadecimal: %q", traceID)
			}
			if traceID == item.forbidden {
				t.Fatalf("invalid carrier retained forbidden trace ID %q", traceID)
			}
			if got := response.Header().Get("Traceparent"); got != "" {
				t.Fatalf("unexpected response traceparent %q", got)
			}
		})
	}
}
"#,
    );

    replace_exact(
        "src/rust/src/pipeline.rs",
        r#"    #[test]
    fn trace_id_parser_rejects_zero_and_non_hex_values() {
        let zero = "00-00000000000000000000000000000000-0123456789abcdef-01".to_string();
        let invalid = "00-zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz-0123456789abcdef-01".to_string();
        let valid = "00-0123456789ABCDEF0123456789ABCDEF-0123456789abcdef-01".to_string();

        assert_eq!(parse_trace_id(Some(&zero)), None);
        assert_eq!(parse_trace_id(Some(&invalid)), None);
        assert_eq!(
            parse_trace_id(Some(&valid)).as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
    }"#,
        r#"    #[test]
    fn trace_id_parser_accepts_only_a_complete_valid_version_zero_parent() {
        let invalid = [
            "00-00000000000000000000000000000000-0123456789abcdef-01",
            "00-0123456789abcdef0123456789abcdef-0000000000000000-01",
            "00-0123456789abcdef0123456789abcdef-zzzzzzzzzzzzzzzz-01",
            "ff-0123456789abcdef0123456789abcdef-0123456789abcdef-01",
            "00-0123456789abcdef0123456789abcdef-0123456789abcdef-zz",
            "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01-extra",
        ]
        .map(str::to_string);
        for value in &invalid {
            assert_eq!(parse_trace_id(Some(value)), None, "{value}");
        }

        let valid =
            "00-0123456789ABCDEF0123456789ABCDEF-0123456789ABCDEF-01".to_string();
        assert_eq!(
            parse_trace_id(Some(&valid)).as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
    }

    #[tokio::test]
    async fn begin_restarts_invalid_traceparent_instead_of_retaining_partial_trace_id() {
        let invalid = [
            "00-0123456789abcdef0123456789abcdef-0000000000000000-01",
            "00-0123456789abcdef0123456789abcdef-zzzzzzzzzzzzzzzz-01",
            "ff-0123456789abcdef0123456789abcdef-0123456789abcdef-01",
            "00-0123456789abcdef0123456789abcdef-0123456789abcdef-zz",
            "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01-extra",
        ];

        let mut config = default_config("traceparent-policy-test");
        config.settings.tls.mode = "in-process".into();
        config.settings.tls.trusted_proxy_cidrs.clear();
        config.settings.rate_limit.enabled = false;
        let stack = MiddlewareStack::new(config).unwrap();

        for value in invalid {
            let mut request = request("198.51.100.10", None, true);
            request.headers.insert("traceparent".into(), value.into());
            let active = stack.begin(request).await.unwrap();
            assert_eq!(active.context.trace_id.len(), 32);
            assert_ne!(
                active.context.trace_id,
                "0123456789abcdef0123456789abcdef",
                "invalid carrier retained its partial trace ID: {value}"
            );
            stack.finish(active, 204, None).await;
        }
    }"#,
    );

    write(
        "src/gleam/test/traceparent_policy_test.gleam",
        r#"import gleam/dict
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

fn request_with_traceparent(traceparent: String) -> ores_middleware.Request {
  ores_middleware.Request(
    method: "GET",
    path: "/trace",
    scheme: "http",
    headers: dict.from_list([
      #("accept", "application/json"),
      #("traceparent", traceparent),
    ]),
    body_size: 0,
    remote_ip: "127.0.0.1",
  )
}

fn request(trace_id: String) -> ores_middleware.Request {
  request_with_traceparent(
    "00-" <> trace_id <> "-" <> valid_parent_span_id <> "-01",
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

fn assert_invalid_inbound(traceparent: String, forbidden_trace_id: String) {
  let response =
    middleware()(request_with_traceparent(traceparent), fn(_) {
      let assert Ok(context) = ores_middleware.current_context()
      assert context.trace_id != forbidden_trace_id
      assert string.length(context.trace_id) == 32
      ores_middleware.Response(204, dict.new(), "")
    })

  assert response.status == 204
  assert dict.get(response.headers, "traceparent") == Error(Nil)
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

pub fn invalid_inbound_traceparent_restarts_entire_trace_test() {
  assert_invalid_inbound(
    "00-" <> zero_trace_id <> "-" <> valid_parent_span_id <> "-01",
    zero_trace_id,
  )
  assert_invalid_inbound(
    "00-" <> valid_trace_id <> "-0000000000000000-01",
    valid_trace_id,
  )
  assert_invalid_inbound(
    "00-" <> valid_trace_id <> "-zzzzzzzzzzzzzzzz-01",
    valid_trace_id,
  )
  assert_invalid_inbound(
    "ff-" <> valid_trace_id <> "-" <> valid_parent_span_id <> "-01",
    valid_trace_id,
  )
  assert_invalid_inbound(
    "00-" <> valid_trace_id <> "-" <> valid_parent_span_id <> "-zz",
    valid_trace_id,
  )
  assert_invalid_inbound(
    "00-" <> valid_trace_id <> "-" <> valid_parent_span_id <> "-01-extra",
    valid_trace_id,
  )
}
"#,
    );

    write(
        "src/elixir/test/traceparent_policy_test.exs",
        r#"defmodule OresMiddleware.TraceparentPolicyTest do
  use ExUnit.Case, async: false

  import Plug.Conn
  import Plug.Test, only: [conn: 2]

  @zero_trace_id String.duplicate("0", 32)
  @valid_trace_id "0123456789abcdef0123456789abcdef"
  @valid_parent_span_id "0123456789abcdef"
  @valid_server_span_id "fedcba9876543210"

  defp stack do
    config = OresMiddleware.default_config("traceparent-policy-test")
    config = put_in(config, [:environment], :test)
    config = put_in(config, [:settings, :tls, :requireHttps], false)
    config = put_in(config, [:settings, :rateLimit, :enabled], false)
    config = put_in(config, [:settings, :idempotency, :enabled], false)
    OresMiddleware.Stack.new!(config)
  end

  defp request_with_traceparent(value) do
    conn(:get, "/trace")
    |> put_req_header("accept", "application/json")
    |> put_req_header("traceparent", value)
  end

  defp request(trace_id) do
    request_with_traceparent("00-#{trace_id}-#{@valid_parent_span_id}-01")
  end

  test "the inbound parent is not relabelled as a response server span" do
    response =
      OresMiddleware.Plug.wrap(stack(), request(@valid_trace_id), fn conn ->
        resp(conn, 204, "")
      end)

    assert get_resp_header(response, "traceparent") == []
  end

  test "only a valid tracer-owned response traceparent is preserved" do
    valid = "00-#{@valid_trace_id}-#{@valid_server_span_id}-01"

    valid_response =
      OresMiddleware.Plug.wrap(stack(), request(@valid_trace_id), fn conn ->
        conn
        |> put_resp_header("traceparent", String.upcase(valid))
        |> resp(204, "")
      end)

    assert get_resp_header(valid_response, "traceparent") == [valid]

    invalid_response =
      OresMiddleware.Plug.wrap(stack(), request(@valid_trace_id), fn conn ->
        conn
        |> put_resp_header(
          "traceparent",
          "00-#{@valid_trace_id}-0000000000000000-01"
        )
        |> resp(204, "")
      end)

    assert get_resp_header(invalid_response, "traceparent") == []
  end

  test "invalid inbound traceparent restarts the entire trace" do
    cases = [
      {"zero trace", "00-#{@zero_trace_id}-#{@valid_parent_span_id}-01", @zero_trace_id},
      {"zero parent", "00-#{@valid_trace_id}-0000000000000000-01", @valid_trace_id},
      {"non-hex parent", "00-#{@valid_trace_id}-zzzzzzzzzzzzzzzz-01", @valid_trace_id},
      {"unsupported version", "ff-#{@valid_trace_id}-#{@valid_parent_span_id}-01", @valid_trace_id},
      {"non-hex flags", "00-#{@valid_trace_id}-#{@valid_parent_span_id}-zz", @valid_trace_id},
      {"extra field", "00-#{@valid_trace_id}-#{@valid_parent_span_id}-01-extra", @valid_trace_id}
    ]

    Enum.each(cases, fn {name, candidate, forbidden_trace_id} ->
      parent = self()
      ref = make_ref()

      response =
        OresMiddleware.Plug.wrap(stack(), request_with_traceparent(candidate), fn conn ->
          send(parent, {ref, OresMiddleware.current_context().trace_id})
          resp(conn, 204, "")
        end)

      assert_receive {^ref, trace_id}
      assert Regex.match?(~r/^[0-9a-f]{32}$/, trace_id), name
      assert trace_id != forbidden_trace_id, name
      assert response.status == 204, name
      assert get_resp_header(response, "traceparent") == [], name
    end)
  end
end
"#,
    );

    write(
        "src/erlang/test/ores_middleware_traceparent_tests.erl",
        r#"-module(ores_middleware_traceparent_tests).

-include_lib("eunit/include/eunit.hrl").

-define(ZERO_TRACE_ID, <<"00000000000000000000000000000000">>).
-define(VALID_TRACE_ID, <<"0123456789abcdef0123456789abcdef">>).
-define(VALID_PARENT_SPAN_ID, <<"0123456789abcdef">>).
-define(VALID_SERVER_SPAN_ID, <<"fedcba9876543210">>).

config() ->
    Config0 = ores_middleware:default_config(<<"traceparent-policy-test">>),
    Settings0 = maps:get(settings, Config0),
    Tls0 = maps:get(tls, Settings0),
    Rate0 = maps:get(rate_limit, Settings0),
    Idempotency0 = maps:get(idempotency, Settings0),
    Config0#{
        environment => test,
        settings => Settings0#{
            tls => Tls0#{require_https => false},
            rate_limit => Rate0#{enabled => false},
            idempotency => Idempotency0#{enabled => false}
        }
    }.

request_with_traceparent(Value) ->
    #{
        method => <<"GET">>,
        path => <<"/trace">>,
        scheme => <<"http">>,
        headers => #{
            <<"accept">> => <<"application/json">>,
            <<"traceparent">> => Value
        },
        body_size => 0,
        remote_ip => <<"127.0.0.1">>
    }.

request(TraceId) ->
    request_with_traceparent(
        <<"00-", TraceId/binary, "-", ?VALID_PARENT_SPAN_ID/binary, "-01">>
    ).

middleware() ->
    {ok, Middleware} = ores_middleware:create_middleware(config(), #{}),
    Middleware.

assert_invalid_inbound(Value, ForbiddenTraceId) ->
    Middleware = middleware(),
    Parent = self(),
    Ref = make_ref(),
    Response = Middleware(request_with_traceparent(Value), fun(_Request) ->
        Parent ! {Ref, ores_middleware:current_trace_id()},
        #{status => 204, headers => #{}, body => <<>>}
    end),
    ?assertEqual(204, maps:get(status, Response)),
    receive
        {Ref, TraceId} ->
            ?assertNotEqual(ForbiddenTraceId, TraceId),
            ?assertEqual(32, byte_size(TraceId))
    after 1000 ->
        ?assert(false)
    end,
    ?assertEqual(false, maps:is_key(<<"traceparent">>, maps:get(headers, Response))).

inbound_parent_is_not_relabelled_as_response_span_test() ->
    Middleware = middleware(),
    Response = Middleware(request(?VALID_TRACE_ID), fun(_Request) ->
        #{status => 204, headers => #{}, body => <<>>}
    end),
    ?assertEqual(false, maps:is_key(<<"traceparent">>, maps:get(headers, Response))).

only_valid_tracer_owned_response_traceparent_is_preserved_test() ->
    Middleware = middleware(),
    Valid = <<"00-", ?VALID_TRACE_ID/binary, "-", ?VALID_SERVER_SPAN_ID/binary, "-01">>,
    ValidResponse = Middleware(request(?VALID_TRACE_ID), fun(_Request) ->
        #{status => 204, headers => #{<<"traceparent">> => string:uppercase(Valid)}, body => <<>>}
    end),
    ?assertEqual(Valid, maps:get(<<"traceparent">>, maps:get(headers, ValidResponse))),

    Invalid = <<"00-", ?VALID_TRACE_ID/binary, "-0000000000000000-01">>,
    InvalidResponse = Middleware(request(?VALID_TRACE_ID), fun(_Request) ->
        #{status => 204, headers => #{<<"traceparent">> => Invalid}, body => <<>>}
    end),
    ?assertEqual(false, maps:is_key(<<"traceparent">>, maps:get(headers, InvalidResponse))).

invalid_inbound_traceparent_restarts_entire_trace_test() ->
    assert_invalid_inbound(
        <<"00-", ?ZERO_TRACE_ID/binary, "-", ?VALID_PARENT_SPAN_ID/binary, "-01">>,
        ?ZERO_TRACE_ID
    ),
    assert_invalid_inbound(
        <<"00-", ?VALID_TRACE_ID/binary, "-0000000000000000-01">>,
        ?VALID_TRACE_ID
    ),
    assert_invalid_inbound(
        <<"00-", ?VALID_TRACE_ID/binary, "-zzzzzzzzzzzzzzzz-01">>,
        ?VALID_TRACE_ID
    ),
    assert_invalid_inbound(
        <<"ff-", ?VALID_TRACE_ID/binary, "-", ?VALID_PARENT_SPAN_ID/binary, "-01">>,
        ?VALID_TRACE_ID
    ),
    assert_invalid_inbound(
        <<"00-", ?VALID_TRACE_ID/binary, "-", ?VALID_PARENT_SPAN_ID/binary, "-zz">>,
        ?VALID_TRACE_ID
    ),
    assert_invalid_inbound(
        <<"00-", ?VALID_TRACE_ID/binary, "-", ?VALID_PARENT_SPAN_ID/binary, "-01-extra">>,
        ?VALID_TRACE_ID
    ).
"#,
    );

    println!("applied strict inbound traceparent validation");
}

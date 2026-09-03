use std::{
    fs,
    path::{Path, PathBuf},
};

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

fn write(path: &str, content: &str) {
    fs::write(path, content).unwrap_or_else(|error| panic!("failed to write {path}: {error}"));
}

fn replace_exact(path: &str, old: &str, new: &str) {
    let content = read(path);
    let matches = content.match_indices(old).count();
    assert_eq!(
        matches, 1,
        "expected exactly one match in {path}, found {matches}"
    );
    write(path, &content.replacen(old, new, 1));
}

fn insert_before_last(path: &str, marker: &str, addition: &str) {
    let content = read(path);
    let position = content
        .rfind(marker)
        .unwrap_or_else(|| panic!("missing final marker in {path}"));
    let mut updated = String::with_capacity(content.len() + addition.len());
    updated.push_str(&content[..position]);
    updated.push_str(addition);
    updated.push_str(&content[position..]);
    write(path, &updated);
}

fn create_text(path: &str, content: &str) {
    let path_buf = PathBuf::from(path);
    assert!(!path_buf.exists(), "refusing to overwrite new file {path}");
    if let Some(parent) = path_buf.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("failed to create {}: {error}", parent.display()));
    }
    write(path, content);
}

fn main() {
    replace_exact(
        "src/ts/src/index.ts",
        r#"function parseTraceId(value: string | null): string | undefined { const part = value?.split("-")[1]; return part && /^[0-9a-fA-F]{32}$/.test(part) ? part.toLowerCase() : undefined; }
function problem(status: number, code: string, detail: string): Response { return Response.json({ type: `urn:ores:middleware:${code}`, title: code, status, detail }, { status, headers: { "content-type": "application/problem+json" } }); }"#,
        r#"function parseTraceId(value: string | null): string | undefined {
  const part = value?.split("-")[1]?.toLowerCase();
  return part &&
    /^[0-9a-f]{32}$/.test(part) &&
    part !== "00000000000000000000000000000000"
    ? part
    : undefined;
}

function validTraceparent(value: string | null): string | undefined {
  if (!value) return undefined;
  const parts = value.split("-");
  if (parts.length !== 4) return undefined;

  const version = parts[0]?.toLowerCase();
  const traceId = parts[1]?.toLowerCase();
  const spanId = parts[2]?.toLowerCase();
  const flags = parts[3]?.toLowerCase();
  if (
    version !== "00" ||
    !traceId ||
    !/^[0-9a-f]{32}$/.test(traceId) ||
    traceId === "00000000000000000000000000000000" ||
    !spanId ||
    !/^[0-9a-f]{16}$/.test(spanId) ||
    spanId === "0000000000000000" ||
    !flags ||
    !/^[0-9a-f]{2}$/.test(flags)
  ) {
    return undefined;
  }
  return `${version}-${traceId}-${spanId}-${flags}`;
}

function problem(status: number, code: string, detail: string): Response { return Response.json({ type: `urn:ores:middleware:${code}`, title: code, status, detail }, { status, headers: { "content-type": "application/problem+json" } }); }"#,
    );

    replace_exact(
        "src/ts/src/index.ts",
        r#"  const headers = new Headers(response.headers);
  headers.set(config.settings.requestIdHeader, context.requestId);
  headers.set("traceparent", `00-${context.traceId}-0000000000000000-01`);
  headers.append("vary", "accept, accept-encoding");"#,
        r#"  const headers = new Headers(response.headers);
  headers.set(config.settings.requestIdHeader, context.requestId);
  const responseTraceparent = validTraceparent(headers.get("traceparent"));
  if (responseTraceparent) headers.set("traceparent", responseTraceparent);
  else headers.delete("traceparent");
  headers.append("vary", "accept, accept-encoding");"#,
    );

    replace_exact(
        "src/ts/src/adapters.ts",
        r#"  /**
   * Response trace header. It is emitted only when an inbound/non-zero span ID
   * is available; the adapter never invents an invalid all-zero W3C span ID.
   */
  traceparentResponseHeader?: string;"#,
        r#"  /**
   * Reserved for compatibility. Response trace context is owned by the active
   * tracer; this adapter never echoes an inbound parent or invents a span.
   */
  traceparentResponseHeader?: string;"#,
    );

    replace_exact(
        "src/ts/src/adapters.ts",
        r#"function validSpanId(value: string | undefined): string | undefined {
  return value && /^[0-9a-f]{16}$/i.test(value) && value !== "0000000000000000"
    ? value.toLowerCase()
    : undefined;
}

function inboundSpanId(request: Request): string | undefined {
  return validSpanId(request.headers.get("traceparent")?.split("-")[2]);
}

"#,
        "",
    );

    replace_exact(
        "src/ts/src/adapters.ts",
        r#"function applyEarlyCorrelationHeaders(
  res: any,
  context: RequestContext | undefined,
  request: Request,
  options: ExpressAdapterOptions
): void {
  if (!context || res.headersSent || typeof res.setHeader !== "function") return;
  res.setHeader(options.requestIdResponseHeader ?? "x-request-id", context.requestId);

  const spanId = validSpanId(context.spanId) ?? inboundSpanId(request);
  if (spanId) {
    res.setHeader(
      options.traceparentResponseHeader ?? "traceparent",
      `00-${context.traceId}-${spanId}-01`
    );
  }
}"#,
        r#"function applyEarlyCorrelationHeaders(
  res: any,
  context: RequestContext | undefined,
  options: ExpressAdapterOptions
): void {
  if (!context || res.headersSent || typeof res.setHeader !== "function") return;
  res.setHeader(options.requestIdResponseHeader ?? "x-request-id", context.requestId);
  // A response traceparent belongs to a real active server span. The portable
  // adapter deliberately leaves that header to the runtime tracer.
}"#,
    );

    replace_exact(
        "src/ts/src/adapters.ts",
        "        applyEarlyCorrelationHeaders(res, context, scopedRequest, options);",
        "        applyEarlyCorrelationHeaders(res, context, options);",
    );

    replace_exact(
        "src/ts/test/otel-adversarial.test.mjs",
        r#"  assert.equal(response.headers.get("traceparent"), `00-${traceId}-0000000000000000-01`);"#,
        r#"  assert.equal(response.headers.get("traceparent"), null);"#,
    );

    replace_exact(
        "src/ts/test/node-adapters.test.mjs",
        r#"    assert.equal(result.response.getHeader("x-request-id"), `request-${slot}`);

    for (const message of [`express-file:${slot}`, `express-request:${slot}`]) {"#,
        r#"    assert.equal(result.response.getHeader("x-request-id"), `request-${slot}`);
    assert.equal(
      result.response.getHeader("traceparent"),
      undefined,
      "the inbound parent span must not be relabelled as a server span"
    );

    for (const message of [`express-file:${slot}`, `express-request:${slot}`]) {"#,
    );

    create_text(
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

function request(traceId = VALID_TRACE_ID) {
  return new Request("http://example.test/trace", {
    headers: {
      accept: "application/json",
      traceparent: `00-${traceId}-${VALID_PARENT_SPAN_ID}-01`
    }
  });
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

test("an all-zero inbound trace ID is replaced and never propagated", async () => {
  const middleware = createMiddleware(testConfig());
  let observedTraceId;

  const response = await middleware(request(ZERO_TRACE_ID), async () => {
    observedTraceId = currentContext()?.traceId;
    return new Response(null, { status: 204 });
  });

  assert.match(observedTraceId, /^[0-9a-f]{32}$/);
  assert.notEqual(observedTraceId, ZERO_TRACE_ID);
  assert.equal(response.headers.get("traceparent"), null);
});
"#,
    );

    replace_exact(
        "src/golang/middleware.go",
        r#"		headers.Set(s.config.Settings.RequestIDHeader, value.RequestID)
		headers.Set("Traceparent", "00-"+value.TraceID+"-0000000000000000-01")
		headers.Add("Vary", "Accept")"#,
        r#"		headers.Set(s.config.Settings.RequestIDHeader, value.RequestID)
		if traceparent := validTraceparent(headers.Get("Traceparent")); traceparent != "" {
			headers.Set("Traceparent", traceparent)
		} else {
			headers.Del("Traceparent")
		}
		headers.Add("Vary", "Accept")"#,
    );

    replace_exact(
        "src/golang/middleware.go",
        r#"func parseTraceID(value string) string {
	parts := strings.Split(value, "-")
	if len(parts) < 2 || len(parts[1]) != 32 {
		return ""
	}
	if _, err := hex.DecodeString(parts[1]); err != nil {
		return ""
	}
	return strings.ToLower(parts[1])
}
func acceptsAny(accept string, supported []string) bool {"#,
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
}

func validTraceparent(value string) string {
	parts := strings.Split(value, "-")
	if len(parts) != 4 || strings.ToLower(parts[0]) != "00" {
		return ""
	}
	traceID := strings.ToLower(parts[1])
	spanID := strings.ToLower(parts[2])
	flags := strings.ToLower(parts[3])
	if len(traceID) != 32 || traceID == strings.Repeat("0", 32) ||
		len(spanID) != 16 || spanID == strings.Repeat("0", 16) ||
		len(flags) != 2 {
		return ""
	}
	for _, part := range []string{traceID, spanID, flags} {
		if _, err := hex.DecodeString(part); err != nil {
			return ""
		}
	}
	return strings.Join([]string{"00", traceID, spanID, flags}, "-")
}

func acceptsAny(accept string, supported []string) bool {"#,
    );

    replace_exact(
        "src/golang/middleware_test.go",
        r#"	if response.Header().Get("x-request-id") == "" || response.Header().Get("traceparent") == "" {
		t.Fatal("missing correlation headers")
	}"#,
        r#"	if response.Header().Get("x-request-id") == "" {
		t.Fatal("missing request ID response header")
	}
	if response.Header().Get("traceparent") != "" {
		t.Fatal("middleware must not synthesize a response traceparent without a server span")
	}"#,
    );

    create_text(
        "src/golang/traceparent_policy_test.go",
        r#"package oresmiddleware

import (
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

func traceparentRequest(traceID string) *http.Request {
	request := httptest.NewRequest(http.MethodGet, "http://example.test/trace", nil)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("Traceparent", "00-"+traceID+"-"+validParentSpanID+"-01")
	return request
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

func TestAllZeroInboundTraceIDIsReplaced(t *testing.T) {
	stack, err := New(testConfig(), Dependencies{})
	if err != nil {
		t.Fatal(err)
	}
	observed := make(chan string, 1)
	handler := stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		context, ok := CurrentContext(request.Context())
		if !ok {
			t.Fatal("request context missing")
		}
		observed <- context.TraceID
		writer.WriteHeader(http.StatusNoContent)
	}))
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, traceparentRequest(zeroTraceID))

	traceID := <-observed
	if traceID == zeroTraceID || len(traceID) != 32 {
		t.Fatalf("invalid replacement trace ID %q", traceID)
	}
	if got := response.Header().Get("Traceparent"); got != "" {
		t.Fatalf("unexpected response traceparent %q", got)
	}
}
"#,
    );

    replace_exact(
        "src/rust/src/pipeline.rs",
        r#"        headers.insert(
            "traceparent".into(),
            format!(
                "00-{}-0000000000000000-01",
                active.context.trace_id
            ),
        );
"#,
        "",
    );

    replace_exact(
        "src/rust/src/pipeline.rs",
        r#"fn parse_trace_id(header: Option<&String>) -> Option<String> {
    let value = header?;
    let mut parts = value.split('-');
    let _version = parts.next()?;
    let trace_id = parts.next()?;
    (trace_id.len() == 32 && trace_id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| trace_id.to_ascii_lowercase())
}"#,
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
    );

    insert_before_last(
        "src/rust/src/pipeline.rs",
        "\n}",
        r#"

    #[test]
    fn trace_id_parser_rejects_zero_and_non_hex_values() {
        let zero =
            "00-00000000000000000000000000000000-0123456789abcdef-01".to_string();
        let invalid =
            "00-zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz-0123456789abcdef-01".to_string();
        let valid =
            "00-0123456789ABCDEF0123456789ABCDEF-0123456789abcdef-01".to_string();

        assert_eq!(parse_trace_id(Some(&zero)), None);
        assert_eq!(parse_trace_id(Some(&invalid)), None);
        assert_eq!(
            parse_trace_id(Some(&valid)).as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
    }

    #[tokio::test]
    async fn finish_does_not_synthesize_response_traceparent_without_server_span() {
        let mut config = default_config("traceparent-policy-test");
        config.settings.tls.mode = "in-process".into();
        config.settings.tls.trusted_proxy_cidrs.clear();
        config.settings.rate_limit.enabled = false;
        let stack = MiddlewareStack::new(config).unwrap();

        let active = stack
            .begin(request("198.51.100.10", None, true))
            .await
            .unwrap();
        let headers = stack.finish(active, 204, None).await;

        assert!(!headers.contains_key("traceparent"));
    }
"#,
    );

    replace_exact(
        "src/gleam/src/ores_middleware.gleam",
        r#"  let headers =
    response.headers
    |> dict.insert(config.request_id_header, context.request_id)
    |> dict.insert(
      "traceparent",
      "00-" <> context.trace_id <> "-0000000000000000-01",
    )
  let headers = case config.security_headers_enabled {"#,
        r#"  let headers =
    response.headers
    |> dict.insert(config.request_id_header, context.request_id)
  let headers = case dict.get(headers, "traceparent") {
    Ok(value) ->
      case normalize_traceparent(value) {
        Ok(value) -> dict.insert(headers, "traceparent", value)
        Error(_) -> dict.delete(headers, "traceparent")
      }
    Error(_) -> headers
  }
  let headers = case config.security_headers_enabled {"#,
    );

    replace_exact(
        "src/gleam/src/ores_middleware.gleam",
        r#"fn trace_id(value: String) -> String {
  case string.split(value, "-") {
    [_, trace, ..] ->
      case string.length(trace) == 32 {
        True -> trace
        False -> ""
      }
    _ -> ""
  }
}

fn value_or_new(value: String) -> String {"#,
        r#"fn trace_id(value: String) -> String {
  case string.split(value, "-") {
    [_, trace, ..] ->
      case valid_hex_identifier(
        trace,
        32,
        "00000000000000000000000000000000",
      ) {
        True -> string.lowercase(trace)
        False -> ""
      }
    _ -> ""
  }
}

fn normalize_traceparent(value: String) -> Result(String, Nil) {
  case string.split(value, "-") {
    [version, trace, span, flags] -> {
      let version = string.lowercase(version)
      let trace = string.lowercase(trace)
      let span = string.lowercase(span)
      let flags = string.lowercase(flags)
      case
        version == "00"
        && valid_hex_identifier(
          trace,
          32,
          "00000000000000000000000000000000",
        )
        && valid_hex_identifier(span, 16, "0000000000000000")
        && valid_hex(flags, 2)
      {
        True -> Ok(version <> "-" <> trace <> "-" <> span <> "-" <> flags)
        False -> Error(Nil)
      }
    }
    _ -> Error(Nil)
  }
}

fn valid_hex_identifier(
  value: String,
  expected_length: Int,
  zero_value: String,
) -> Bool {
  value != zero_value && valid_hex(value, expected_length)
}

fn valid_hex(value: String, expected_length: Int) -> Bool {
  string.length(value) == expected_length
  && list.all(string.to_graphemes(value), fn(character) {
    string.contains("0123456789abcdefABCDEF", character)
  })
}

fn value_or_new(value: String) -> String {"#,
    );

    create_text(
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
  let valid =
    "00-" <> valid_trace_id <> "-" <> valid_server_span_id <> "-01"
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
          #(
            "traceparent",
            "00-" <> valid_trace_id <> "-0000000000000000-01",
          ),
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
"#,
    );

    replace_exact(
        "src/elixir/lib/ores_middleware/plug.ex",
        r#"  defp attach_correlation(conn, config, context), do: conn |> put_resp_header(config.settings.requestIdHeader, context.request_id) |> put_resp_header("traceparent", "00-#{context.trace_id}-0000000000000000-01") |> update_resp_header("vary", "accept", &(&1 <> ", accept"))"#,
        r#"  defp attach_correlation(conn, config, context) do
    conn
    |> put_resp_header(config.settings.requestIdHeader, context.request_id)
    |> sanitize_traceparent()
    |> update_resp_header("vary", "accept", &(&1 <> ", accept"))
  end"#,
    );

    replace_exact(
        "src/elixir/lib/ores_middleware/plug.ex",
        r#"  defp parse_trace_id(value) when is_binary(value) do
    case String.split(value, "-") do [_, trace | _] -> if(String.match?(trace, ~r/^[0-9a-fA-F]{32}$/), do: String.downcase(trace)); _ -> nil end
  end
  defp parse_trace_id(_), do: nil
  defp random_id, do: :crypto.strong_rand_bytes(16) |> Base.encode16(case: :lower)"#,
        r#"  defp parse_trace_id(value) when is_binary(value) do
    case String.split(value, "-") do
      [_, trace | _] ->
        trace = String.downcase(trace)
        if valid_hex_id?(trace, 32, String.duplicate("0", 32)), do: trace
      _ -> nil
    end
  end
  defp parse_trace_id(_), do: nil

  defp sanitize_traceparent(conn) do
    case get_resp_header(conn, "traceparent") do
      [value | _] ->
        case normalize_traceparent(value) do
          nil -> delete_resp_header(conn, "traceparent")
          normalized -> put_resp_header(conn, "traceparent", normalized)
        end
      [] -> conn
    end
  end

  defp normalize_traceparent(value) when is_binary(value) do
    case String.split(value, "-") do
      [version, trace, span, flags] ->
        version = String.downcase(version)
        trace = String.downcase(trace)
        span = String.downcase(span)
        flags = String.downcase(flags)

        if version == "00" and
             valid_hex_id?(trace, 32, String.duplicate("0", 32)) and
             valid_hex_id?(span, 16, String.duplicate("0", 16)) and
             Regex.match?(~r/^[0-9a-f]{2}$/, flags) do
          Enum.join([version, trace, span, flags], "-")
        end
      _ -> nil
    end
  end
  defp normalize_traceparent(_), do: nil

  defp valid_hex_id?(value, length, zero) do
    value != zero and byte_size(value) == length and Regex.match?(~r/^[0-9a-f]+$/, value)
  end

  defp random_id, do: :crypto.strong_rand_bytes(16) |> Base.encode16(case: :lower)"#,
    );

    replace_exact(
        "src/elixir/test/ores_middleware_test.exs",
        r#"    assert Plug.Conn.get_resp_header(conn, "traceparent") != []"#,
        r#"    assert Plug.Conn.get_resp_header(conn, "traceparent") == []"#,
    );

    create_text(
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

  defp request(trace_id) do
    conn(:get, "/trace")
    |> put_req_header("accept", "application/json")
    |> put_req_header(
      "traceparent",
      "00-#{trace_id}-#{@valid_parent_span_id}-01"
    )
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

  test "an all-zero inbound trace ID is replaced" do
    response =
      OresMiddleware.Plug.wrap(stack(), request(@zero_trace_id), fn conn ->
        context = OresMiddleware.current_context()
        assert context.trace_id != @zero_trace_id
        assert context.trace_id =~ ~r/^[0-9a-f]{32}$/
        resp(conn, 204, "")
      end)

    assert response.status == 204
    assert get_resp_header(response, "traceparent") == []
  end
end
"#,
    );

    replace_exact(
        "src/erlang/src/ores_middleware.erl",
        r#"    Headers1 = Headers0#{maps:get(request_id_header, Settings) => maps:get(request_id, Context), <<"traceparent">> => <<"00-", (maps:get(trace_id, Context))/binary, "-0000000000000000-01">>},"#,
        r#"    Headers1 = sanitize_traceparent(Headers0#{maps:get(request_id_header, Settings) => maps:get(request_id, Context)}),"#,
    );

    replace_exact(
        "src/erlang/src/ores_middleware.erl",
        r#"trace_or_new(Value) when is_binary(Value) -> case binary:split(Value, <<"-">>, [global]) of [_, Trace | _] when byte_size(Trace) =:= 32 -> string:lowercase(Trace); _ -> new_id() end;
trace_or_new(_) -> new_id().
new_id() -> binary:encode_hex(crypto:strong_rand_bytes(16), lowercase)."#,
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
trace_or_new(_) -> new_id().

sanitize_traceparent(Headers) ->
    case maps:get(<<"traceparent">>, Headers, undefined) of
        Value when is_binary(Value) ->
            case normalize_traceparent(Value) of
                {ok, Normalized} -> Headers#{<<"traceparent">> => Normalized};
                error -> maps:remove(<<"traceparent">>, Headers)
            end;
        _ -> maps:remove(<<"traceparent">>, Headers)
    end.

normalize_traceparent(Value) ->
    case binary:split(Value, <<"-">>, [global]) of
        [Version0, Trace0, Span0, Flags0] ->
            Version = string:lowercase(Version0),
            Trace = string:lowercase(Trace0),
            Span = string:lowercase(Span0),
            Flags = string:lowercase(Flags0),
            case Version =:= <<"00">>
                andalso valid_hex_id(Trace, 32, <<"00000000000000000000000000000000">>)
                andalso valid_hex_id(Span, 16, <<"0000000000000000">>)
                andalso valid_hex(Flags, 2) of
                true -> {ok, iolist_to_binary([Version, <<"-">>, Trace, <<"-">>, Span, <<"-">>, Flags])};
                false -> error
            end;
        _ -> error
    end.

valid_hex_id(Value, Length, Zero) ->
    Value =/= Zero andalso valid_hex(Value, Length).

valid_hex(Value, Length) when is_binary(Value), byte_size(Value) =:= Length ->
    lists:all(fun(Byte) ->
        (Byte >= $0 andalso Byte =< $9)
            orelse (Byte >= $a andalso Byte =< $f)
    end, binary_to_list(Value));
valid_hex(_, _) -> false.

new_id() -> binary:encode_hex(crypto:strong_rand_bytes(16), lowercase)."#,
    );

    replace_exact(
        "src/erlang/src/ores_middleware_cowboy.erl",
        r#"            ResponseHeaders0 = #{
                maps:get(request_id_header, maps:get(settings, Config)) =>
                    maps:get(request_id, Context),
                <<"traceparent">> =>
                    <<"00-", (maps:get(trace_id, Context))/binary,
                      "-0000000000000000-01">>
            },"#,
        r#"            ResponseHeaders0 = #{
                maps:get(request_id_header, maps:get(settings, Config)) =>
                    maps:get(request_id, Context)
            },"#,
    );

    create_text(
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

request(TraceId) ->
    #{
        method => <<"GET">>,
        path => <<"/trace">>,
        scheme => <<"http">>,
        headers => #{
            <<"accept">> => <<"application/json">>,
            <<"traceparent">> =>
                <<"00-", TraceId/binary, "-", ?VALID_PARENT_SPAN_ID/binary, "-01">>
        },
        body_size => 0,
        remote_ip => <<"127.0.0.1">>
    }.

middleware() ->
    {ok, Middleware} = ores_middleware:create_middleware(config(), #{}),
    Middleware.

inbound_parent_is_not_relabelled_as_response_span_test() ->
    Response = middleware()(request(?VALID_TRACE_ID), fun(_Request) ->
        #{status => 204, headers => #{}, body => <<>>}
    end),
    ?assertEqual(false, maps:is_key(<<"traceparent">>, maps:get(headers, Response))).

only_valid_tracer_owned_response_traceparent_is_preserved_test() ->
    Valid = <<"00-", ?VALID_TRACE_ID/binary, "-", ?VALID_SERVER_SPAN_ID/binary, "-01">>,
    ValidResponse = middleware()(request(?VALID_TRACE_ID), fun(_Request) ->
        #{status => 204, headers => #{<<"traceparent">> => string:uppercase(Valid)}, body => <<>>}
    end),
    ?assertEqual(Valid, maps:get(<<"traceparent">>, maps:get(headers, ValidResponse))),

    Invalid = <<"00-", ?VALID_TRACE_ID/binary, "-0000000000000000-01">>,
    InvalidResponse = middleware()(request(?VALID_TRACE_ID), fun(_Request) ->
        #{status => 204, headers => #{<<"traceparent">> => Invalid}, body => <<>>}
    end),
    ?assertEqual(false, maps:is_key(<<"traceparent">>, maps:get(headers, InvalidResponse))).

all_zero_inbound_trace_id_is_replaced_test() ->
    Parent = self(),
    Response = middleware()(request(?ZERO_TRACE_ID), fun(_Request) ->
        Parent ! {observed_trace_id, ores_middleware:current_trace_id()},
        #{status => 204, headers => #{}, body => <<>>}
    end),
    ?assertEqual(204, maps:get(status, Response)),
    receive
        {observed_trace_id, TraceId} ->
            ?assertNotEqual(?ZERO_TRACE_ID, TraceId),
            ?assertEqual(32, byte_size(TraceId))
    after 1000 ->
        ?assert(false)
    end.
"#,
    );

    replace_exact(
        ".github/workflows/ci.yml",
        r#"      - name: Install exact validators and package dependencies
        shell: bash"#,
        r#"      - name: Enforce response trace-context safety
        shell: bash
        run: |
          set -euo pipefail
          rustc --edition=2024 scripts/check_traceparent_policy.rs -o "${RUNNER_TEMP}/check-traceparent-policy"
          "${RUNNER_TEMP}/check-traceparent-policy"

      - name: Install exact validators and package dependencies
        shell: bash"#,
    );

    create_text(
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
                failures.push(format!("{path}: forbidden response trace-context pattern {forbidden:?}"));
            }
        }
    }

    for (path, required) in [
        ("src/ts/src/index.ts", "function validTraceparent"),
        ("src/golang/middleware.go", "func validTraceparent"),
        ("src/gleam/src/ores_middleware.gleam", "fn normalize_traceparent"),
        ("src/elixir/lib/ores_middleware/plug.ex", "defp normalize_traceparent"),
        ("src/erlang/src/ores_middleware.erl", "normalize_traceparent(Value)"),
    ] {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
        if !content.contains(required) {
            failures.push(format!("{path}: missing required safety marker {required:?}"));
        }
    }

    if !failures.is_empty() {
        eprintln!("response trace-context safety audit failed:");
        for failure in failures {
            eprintln!("- {failure}");
        }
        std::process::exit(1);
    }

    println!("response trace-context safety audit passed");
}
"#,
    );

    create_text(
        "docs/RESPONSE_TRACE_CONTEXT.md",
        r#"# Response trace-context policy

`traceparent` identifies a concrete trace and span. A middleware layer must not
invent an all-zero span ID, and it must not relabel the inbound parent span as
the server span.

## Ownership

- Inbound `traceparent` is parsed only to continue or replace the trace ID.
- A real runtime tracer owns server-span creation, sampling flags, status,
  exception recording, and span completion.
- The portable middleware always emits its configured request-ID response
  header.
- The portable middleware does not synthesize a response `traceparent`.
- A downstream tracer may set a response `traceparent`; middleware preserves it
  only when version, trace ID, span ID, and flags are structurally valid and both
  IDs are non-zero.
- Runtimes whose response API is constructed independently of the handler
  (currently the Rust pipeline metadata API and Cowboy pre-handler adapter) omit
  `traceparent` until their tracer integration can supply the active server
  span.

## Validation

For W3C version `00`, this repository accepts exactly four lowercase-normalized
fields:

```text
00-<32 hex non-zero trace id>-<16 hex non-zero span id>-<2 hex flags>
```

Malformed values and all-zero identifiers fail closed and are removed rather
than propagated.

## Runtime behavior

TypeScript/Fetch, Go, Gleam, Elixir, and Erlang preserve a valid response header
created by the downstream tracer and remove an invalid one. Express and NestJS
set only the request ID before dispatch; they never echo the inbound parent
span. Rust and Cowboy omit response trace context until a tracer-owned server
span is available.

The static Rust audit in `scripts/check_traceparent_policy.rs` runs in CI and
prevents reintroducing the known all-zero response-span pattern.
"#,
    );

    assert!(Path::new("scripts/check_traceparent_policy.rs").exists());
    println!("applied response trace-context repair");
}

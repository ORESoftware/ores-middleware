import test from "node:test";
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

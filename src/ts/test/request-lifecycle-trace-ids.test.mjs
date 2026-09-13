import test from "node:test";
import assert from "node:assert/strict";

import { defaultConfig } from "../dist/index.js";
import { createLogger, createOresOtelMiddleware } from "../dist/otel.js";

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
const settleDetachedLogs = () => delay(25);
const STATIC_TRACE = /^ores-trace-[A-Za-z0-9_-]{12,64}$/;
const W3C_TRACE = "0000000000000000000000000000abcd";

function testConfig(timeoutMs = 5_000) {
  const config = defaultConfig("middleware-lifecycle-trace-test");
  config.environment = "test";
  config.settings.timeoutMs = timeoutMs;
  config.settings.tls.mode = "disabled";
  config.settings.tls.requireHttps = false;
  config.settings.rateLimit.enabled = false;
  config.settings.idempotency.enabled = false;
  config.settings.compression.enabled = false;
  return config;
}

function memoryLogger(records) {
  return createLogger({
    appName: "middleware-lifecycle-trace-test",
    name: "server",
    console: false,
    transports: {
      name: "memory",
      write(record) {
        records.push(record);
      }
    }
  });
}

function tracedRequest() {
  return new Request("http://example.test/orders/1", {
    headers: {
      "x-request-id": "request-lifecycle-1",
      traceparent: `00-${W3C_TRACE}-0123456789abcdef-01`
    }
  });
}

function lifecycleRecord(records, message) {
  // Extra values (such as the handler error) are appended to the record message.
  const record = records.find(
    (candidate) => typeof candidate.message === "string" && candidate.message.startsWith(message)
  );
  assert.ok(record, `missing lifecycle record: ${message}`);
  return record;
}

function staticTraceOf(record) {
  const staticTraces = (record.traceIds ?? []).filter((id) => STATIC_TRACE.test(id));
  assert.equal(staticTraces.length, 1, `expected one static call-site trace on ${record.message}`);
  return staticTraces[0];
}

function assertRequestCorrelationPreserved(record) {
  // The request W3C trace stays primary and the request-scoped routine is not replaced.
  assert.equal(record.traceId, W3C_TRACE);
  assert.ok(record.traceIds.includes(W3C_TRACE));
  assert.equal(record.routineId, "request-lifecycle-1");
  assert.equal(record.fields["request.id"], "request-lifecycle-1");
}

test("started and completed lifecycle records carry distinct static call-site traces", async () => {
  const records = [];
  const middleware = createOresOtelMiddleware(testConfig(), { logger: memoryLogger(records) });

  const response = await middleware(tracedRequest(), async () => new Response("ok"));
  await settleDetachedLogs();

  assert.equal(response.status, 200);
  const started = lifecycleRecord(records, "request handler started");
  const completed = lifecycleRecord(records, "request handler completed");
  assertRequestCorrelationPreserved(started);
  assertRequestCorrelationPreserved(completed);
  assert.notEqual(staticTraceOf(started), staticTraceOf(completed));
});

test("failed lifecycle record carries its own static call-site trace", async () => {
  const records = [];
  const middleware = createOresOtelMiddleware(testConfig(), { logger: memoryLogger(records) });

  // The ores-otel layer logs and rethrows; the portable operation boundary then
  // converts the failure into a typed 500 problem response instead of rejecting.
  const response = await middleware(tracedRequest(), async () => {
    throw new Error("handler exploded");
  });
  await settleDetachedLogs();

  assert.equal(response.status, 500);
  const started = lifecycleRecord(records, "request handler started");
  const failed = lifecycleRecord(records, "request handler failed");
  assertRequestCorrelationPreserved(failed);
  assert.notEqual(staticTraceOf(started), staticTraceOf(failed));
});

test("timeout lifecycle record carries its own static call-site trace", async () => {
  const records = [];
  const middleware = createOresOtelMiddleware(testConfig(15), { logger: memoryLogger(records) });

  await middleware(tracedRequest(), async () => {
    await delay(60);
    return new Response("late");
  });
  await settleDetachedLogs();

  const started = lifecycleRecord(records, "request handler started");
  const timedOut = lifecycleRecord(records, "request handler timed out");
  assertRequestCorrelationPreserved(timedOut);
  assert.notEqual(staticTraceOf(started), staticTraceOf(timedOut));
});

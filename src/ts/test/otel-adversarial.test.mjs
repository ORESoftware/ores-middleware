import test from "node:test";
import assert from "node:assert/strict";

import { defaultConfig } from "../dist/index.js";
import {
  createLogger,
  createOresOtelMiddleware,
  getLogContext,
  requestLogger
} from "../dist/otel.js";

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
const settleDetachedLogs = () => delay(25);

function testConfig(timeoutMs = 5_000) {
  const config = defaultConfig("middleware-adversarial-test");
  config.environment = "test";
  config.settings.timeoutMs = timeoutMs;
  config.settings.tls.mode = "disabled";
  config.settings.tls.requireHttps = false;
  config.settings.rateLimit.enabled = false;
  config.settings.idempotency.enabled = false;
  config.settings.compression.enabled = false;
  return config;
}

function memoryLoggers(records) {
  const transport = {
    name: "memory",
    write(record) {
      records.push(record);
    }
  };
  return {
    root: createLogger({
      appName: "middleware-adversarial-test",
      name: "server",
      console: false,
      transports: transport
    }),
    fileLogger: createLogger({
      appName: "middleware-adversarial-test",
      name: "handler",
      console: false,
      transports: transport
    })
  };
}

function requestFor(slot, extraHeaders = {}) {
  return new Request(`http://example.test/orders/${slot}`, {
    headers: {
      "x-request-id": `request-${slot}`,
      "x-test-slot": String(slot),
      "x-test-user": `user-${slot}`,
      "x-test-tenant": `tenant-${slot}`,
      traceparent: `00-${Number(slot).toString(16).padStart(32, "0")}-0123456789abcdef-01`,
      ...extraHeaders
    }
  });
}

test("parallel requests never cross-contaminate request, user, tenant, or baggage context", async () => {
  const records = [];
  const { root, fileLogger } = memoryLoggers(records);
  const middleware = createOresOtelMiddleware(testConfig(), {
    logger: root,
    authVerifier: async (request) => ({
      userId: request.headers.get("x-test-user"),
      tenantId: request.headers.get("x-test-tenant"),
      claims: {
        "otel.slot": request.headers.get("x-test-slot"),
        authorization: "must-not-propagate"
      }
    })
  });

  const requestCount = 48;
  const responses = await Promise.all(
    Array.from({ length: requestCount }, async (_, slot) =>
      middleware(requestFor(slot), async (request) => {
        await delay((requestCount - slot) % 7);
        const context = getLogContext();
        assert.equal(context?.fields?.["request.id"], `request-${slot}`);
        assert.equal(context?.fields?.["user.id"], `user-${slot}`);
        assert.equal(context?.fields?.["tenant.id"], `tenant-${slot}`);
        assert.equal(context?.fields?.["baggage.otel.slot"], String(slot));

        await fileLogger.info(`file:${slot}`).send();
        const scopedLogger = requestLogger(request);
        assert.ok(scopedLogger);
        await scopedLogger.warn(`request:${slot}`).send();
        return new Response("ok", { status: 200 + (slot % 3) });
      })
    )
  );

  await settleDetachedLogs();
  assert.equal(responses.length, requestCount);
  assert.equal(getLogContext(), undefined);

  for (let slot = 0; slot < requestCount; slot += 1) {
    for (const message of [`file:${slot}`, `request:${slot}`]) {
      const matches = records.filter((record) => record.message === message);
      assert.equal(matches.length, 1, `expected one ${message} record`);
      const [record] = matches;
      assert.equal(record.fields["request.id"], `request-${slot}`);
      assert.equal(record.fields["user.id"], `user-${slot}`);
      assert.equal(record.fields["tenant.id"], `tenant-${slot}`);
      assert.equal(record.fields["baggage.otel.slot"], String(slot));
      assert.equal(record.loggedInUser?.id, `user-${slot}`);
      assert.doesNotMatch(JSON.stringify(record), /must-not-propagate|authorization/i);
    }
  }
});

test("sealed Request objects use the WeakMap logger fallback", async () => {
  const records = [];
  const { root } = memoryLoggers(records);
  const middleware = createOresOtelMiddleware(testConfig(), {
    logger: root,
    authVerifier: async () => ({ userId: "sealed-user", tenantId: "sealed-tenant" })
  });
  const request = requestFor(11, {
    "x-request-id": "request-sealed",
    traceparent: "00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-0123456789abcdef-01"
  });
  Object.preventExtensions(request);

  const response = await middleware(request, async (scopedRequest) => {
    assert.equal(Object.hasOwn(scopedRequest, "log"), false);
    const scopedLogger = requestLogger(scopedRequest);
    assert.ok(scopedLogger);
    await scopedLogger.info("sealed request reached").send();
    return new Response(null, { status: 204 });
  });

  await settleDetachedLogs();
  assert.equal(response.status, 204);
  const record = records.find((candidate) => candidate.message === "sealed request reached");
  assert.equal(record?.fields["request.id"], "request-sealed");
  assert.equal(record?.loggedInUser?.id, "sealed-user");
  assert.equal(getLogContext(), undefined);
});

test("malformed correlation identifiers are replaced and unsafe claims are dropped", async () => {
  const records = [];
  const { root } = memoryLoggers(records);
  const middleware = createOresOtelMiddleware(testConfig(), {
    logger: root,
    authVerifier: async () => ({
      userId: "user-safe",
      tenantId: "tenant-safe",
      claims: {
        "otel.vendor": "allowed",
        authorization: "Bearer must-not-propagate",
        cookie: "must-not-propagate"
      }
    })
  });

  let observed;
  const response = await middleware(
    new Request("http://example.test/malformed", {
      headers: {
        "x-request-id": "bad request id with spaces",
        traceparent: "00-not-a-valid-trace-id-not-a-span-id-01"
      }
    }),
    async (request) => {
      observed = getLogContext();
      const scopedLogger = requestLogger(request);
      assert.ok(scopedLogger);
      await scopedLogger.info("malformed context replaced").send();
      return new Response("ok");
    }
  );

  await settleDetachedLogs();
  assert.equal(response.status, 200);
  const requestId = observed?.fields?.["request.id"];
  const traceId = observed?.fields?.["trace.id"];
  assert.equal(typeof requestId, "string");
  assert.notEqual(requestId, "bad request id with spaces");
  assert.match(requestId, /^[A-Za-z0-9._-]{1,128}$/);
  assert.match(traceId, /^[0-9a-f]{32}$/);
  assert.equal(response.headers.get("x-request-id"), requestId);
  assert.equal(response.headers.get("traceparent"), `00-${traceId}-0000000000000000-01`);

  const record = records.find((candidate) => candidate.message === "malformed context replaced");
  assert.equal(record?.fields["baggage.otel.vendor"], "allowed");
  assert.doesNotMatch(JSON.stringify(record), /Bearer must-not-propagate|cookie|authorization/i);
  assert.equal(getLogContext(), undefined);
});

test("handler failure preserves the 500 response and clears ambient context", async () => {
  const records = [];
  const { root } = memoryLoggers(records);
  const middleware = createOresOtelMiddleware(testConfig(), {
    logger: root,
    authVerifier: async () => ({ userId: "failure-user", tenantId: "failure-tenant" })
  });

  const response = await middleware(requestFor(12), async () => {
    throw new Error("handler exploded");
  });

  await settleDetachedLogs();
  assert.equal(response.status, 500);
  assert.ok(records.some((record) => record.message.startsWith("request handler failed")));
  assert.equal(records.some((record) => record.message === "request handler completed"), false);
  assert.equal(getLogContext(), undefined);
});

test("throwing transport diagnostics cannot prevent the handler or alter its response", async () => {
  let handlerRan = false;
  let transportErrors = 0;
  const root = createLogger({
    appName: "middleware-adversarial-test",
    console: false,
    transports: {
      name: "always-fails",
      write() {
        throw new Error("sink unavailable");
      }
    },
    onTransportError() {
      transportErrors += 1;
      throw new Error("diagnostic callback failed");
    }
  });
  const middleware = createOresOtelMiddleware(testConfig(), {
    logger: root,
    authVerifier: async () => ({ userId: "transport-user" })
  });

  const response = await middleware(requestFor(13), async () => {
    handlerRan = true;
    return new Response(null, { status: 204 });
  });

  await settleDetachedLogs();
  assert.equal(handlerRan, true);
  assert.equal(response.status, 204);
  assert.ok(transportErrors >= 2);
  assert.equal(getLogContext(), undefined);
});

test("a handler that outlives its deadline emits timeout rather than late completion", async () => {
  const records = [];
  const { root } = memoryLoggers(records);
  const middleware = createOresOtelMiddleware(testConfig(15), {
    logger: root,
    authVerifier: async () => ({ userId: "timeout-user", tenantId: "timeout-tenant" })
  });

  const response = await middleware(requestFor(14), async () => {
    await delay(60);
    return new Response(null, { status: 204 });
  });

  assert.equal(response.status, 504);
  await delay(80);
  await settleDetachedLogs();
  assert.ok(records.some((record) => record.message === "request handler timed out"));
  assert.equal(records.some((record) => record.message === "request handler completed"), false);
  assert.equal(getLogContext(), undefined);
});

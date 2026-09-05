import { EventEmitter } from "node:events";
import test from "node:test";
import assert from "node:assert/strict";

import {
  createMiddleware,
  currentContext,
  defaultConfig
} from "../dist/index.js";
import {
  expressMiddleware,
  nestjsMiddleware
} from "../dist/adapters.js";
import { getLogContext } from "@oresoftware/next-loggers/context";

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

function testConfig() {
  const config = defaultConfig("http-rejection-finalization-test");
  config.environment = "test";
  config.settings.tls.mode = "disabled";
  config.settings.tls.requireHttps = false;
  config.settings.rateLimit.enabled = false;
  config.settings.compression.enabled = false;
  config.settings.idempotency.enabled = false;
  return config;
}

function portableRequest(requestId, slot, init = {}) {
  const headers = new Headers({
    accept: "application/json",
    "x-request-id": requestId,
    traceparent: `00-${String(slot + 1).padStart(32, "0")}-0123456789abcdef-01`
  });
  for (const [name, value] of Object.entries(init.headers ?? {})) headers.set(name, value);
  return new Request(`http://example.test/rejections/${slot}`, {
    ...init,
    headers
  });
}

function varyTokens(response) {
  return (response.headers.get("vary") ?? "")
    .split(",")
    .map((value) => value.trim().toLowerCase())
    .filter(Boolean);
}

function assertFinalized(response, requestId, status) {
  assert.equal(response.status, status);
  assert.equal(response.headers.get("x-request-id"), requestId);
  assert.equal(response.headers.get("x-content-type-options"), "nosniff");
  assert.equal(response.headers.get("x-frame-options"), "DENY");
  assert.equal(
    response.headers.get("traceparent"),
    null,
    "an inbound parent span must not be relabelled as a response/server span"
  );
  const tokens = varyTokens(response);
  assert.deepEqual([...new Set(tokens)].sort(), ["accept", "accept-encoding"]);
  assert.equal(tokens.length, 2, "Vary tokens must be unique");
}

test("every ordinary fail-closed response is finalized exactly once", async (t) => {
  const scenarios = [
    {
      name: "payload limit",
      status: 413,
      setup(config) { config.settings.maxBodyBytes = 1; },
      init: { method: "POST", body: "xx", headers: { "content-length": "2" } }
    },
    {
      name: "content negotiation",
      status: 406,
      init: { headers: { accept: "text/plain" } }
    },
    {
      name: "HTTPS requirement",
      status: 426,
      setup(config) { config.settings.tls.requireHttps = true; }
    },
    {
      name: "untrusted forwarded transport",
      status: 400,
      init: { headers: { "x-forwarded-proto": "https" } },
      dependencies: { isTrustedProxy: () => false }
    },
    {
      name: "IP policy",
      status: 403,
      dependencies: { authorizeIp: async () => false }
    },
    {
      name: "rate limit",
      status: 429,
      setup(config) { config.settings.rateLimit.enabled = true; },
      dependencies: { rateLimiter: { allow: async () => false } }
    },
    {
      name: "authentication",
      status: 401,
      setup(config) { config.integrations.sharedAuth.mode = "embedded"; }
    },
    {
      name: "fault injection",
      status: 503,
      setup(config) {
        config.settings.faultInjection.enabled = true;
        config.settings.faultInjection.dropRate = 1;
      },
      dependencies: { random: () => 0 }
    },
    {
      name: "fail-closed sync observer",
      status: 503,
      handlerRuns: 1,
      setup(config) { config.integrations.optoSync.failOpen = false; },
      dependencies: {
        syncObserver: async () => { throw new Error("private sync detail"); }
      }
    }
  ];

  for (const [slot, scenario] of scenarios.entries()) {
    await t.test(scenario.name, async () => {
      const config = testConfig();
      scenario.setup?.(config);
      let handlerRuns = 0;
      const middleware = createMiddleware(config, scenario.dependencies ?? {});
      const response = await middleware(
        portableRequest(`reject-${slot}`, slot, scenario.init),
        async () => {
          handlerRuns += 1;
          return new Response("ok", {
            headers: { vary: "Accept-Encoding, ACCEPT, accept-encoding" }
          });
        }
      );
      assertFinalized(response, `reject-${slot}`, scenario.status);
      assert.equal(handlerRuns, scenario.handlerRuns ?? 0);
      assert.equal(currentContext(), undefined);
      assert.equal(getLogContext(), undefined);
    });
  }
});

test("a valid tracer-owned response span is preserved while Vary is canonicalized", async () => {
  const responseTraceparent = `00-${"a".repeat(32)}-${"b".repeat(16)}-01`;
  const middleware = createMiddleware(testConfig());
  const response = await middleware(portableRequest("handler-success", 50), async () =>
    new Response("ok", {
      headers: {
        traceparent: responseTraceparent,
        vary: "Accept-Encoding, ACCEPT, accept-encoding"
      }
    })
  );

  assert.equal(response.status, 200);
  assert.equal(response.headers.get("x-request-id"), "handler-success");
  assert.equal(response.headers.get("traceparent"), responseTraceparent);
  const tokens = varyTokens(response);
  assert.deepEqual([...new Set(tokens)].sort(), ["accept", "accept-encoding"]);
  assert.equal(tokens.length, 2);
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
});

test("parallel early rejections cannot bleed correlation context", async () => {
  const middleware = createMiddleware(testConfig(), {
    authorizeIp: async (_request, context) => {
      await delay(Number(context.requestId.slice(1)) % 4);
      assert.equal(currentContext()?.requestId, context.requestId);
      assert.equal(getLogContext()?.fields?.["request.id"], context.requestId);
      return false;
    }
  });

  const responses = await Promise.all(
    Array.from({ length: 32 }, (_, slot) =>
      middleware(portableRequest(`r${slot}`, slot), async () => new Response("must not run"))
    )
  );

  for (const [slot, response] of responses.entries()) {
    assertFinalized(response, `r${slot}`, 403);
  }
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
});

class MockResponse extends EventEmitter {
  statusCode = 200;
  headersSent = false;
  writableEnded = false;
  finished = false;
  chunks = [];
  #headers = new Map();

  status(code) { this.statusCode = code; return this; }
  setHeader(name, value) { this.#headers.set(String(name).toLowerCase(), value); return this; }
  getHeader(name) { return this.#headers.get(String(name).toLowerCase()); }
  getHeaders() { return Object.fromEntries(this.#headers); }
  end(body) {
    if (this.writableEnded) return this;
    if (body !== undefined) this.chunks.push(Buffer.from(body));
    this.headersSent = true;
    this.writableEnded = true;
    this.finished = true;
    queueMicrotask(() => this.emit("finish"));
    return this;
  }
  send(body) { return this.end(body); }
}

function nativeRequest(requestId, slot) {
  return {
    method: "GET",
    protocol: "http",
    originalUrl: `/rejections/${slot}`,
    headers: {
      accept: "application/json",
      host: "example.test",
      "x-request-id": requestId,
      traceparent: `00-${String(slot + 1).padStart(32, "0")}-0123456789abcdef-01`
    },
    socket: { encrypted: false }
  };
}

async function runNativeShortCircuit(adapter, requestId, slot) {
  const response = new MockResponse();
  let nextCalls = 0;
  const finished = new Promise((resolve, reject) => {
    response.once("finish", resolve);
    adapter(nativeRequest(requestId, slot), response, (error) => {
      nextCalls += 1;
      reject(error ?? new Error("next must not run for a fail-closed response"));
    });
  });
  await finished;
  assert.equal(nextCalls, 0);
  assert.equal(response.statusCode, 401);
  assert.equal(response.getHeader("x-request-id"), requestId);
  assert.equal(response.getHeader("x-content-type-options"), "nosniff");
  assert.equal(response.getHeader("traceparent"), undefined);
  const tokens = String(response.getHeader("vary") ?? "")
    .split(",")
    .map((value) => value.trim().toLowerCase())
    .filter(Boolean);
  assert.deepEqual([...new Set(tokens)].sort(), ["accept", "accept-encoding"]);
  assert.equal(tokens.length, 2);
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
}

test("Express and NestJS preserve correlation on authentication short-circuits", async () => {
  const config = testConfig();
  config.integrations.sharedAuth.mode = "embedded";
  const middleware = createMiddleware(config);
  await runNativeShortCircuit(expressMiddleware(middleware), "express-reject", 80);
  await runNativeShortCircuit(nestjsMiddleware(middleware), "nestjs-reject", 81);
});


test("the HTTP lifecycle constructs exactly one finalization wrapper", { concurrency: false }, async () => {
  const request = portableRequest("single-finalizer", 90, { method: "POST" });
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "Response");
  assert.ok(descriptor && "value" in descriptor, "Response must be a replaceable data property in Node");
  const NativeResponse = descriptor.value;
  let constructions = 0;

  class CountingResponse extends NativeResponse {
    constructor(body, init) {
      super(body, init);
      constructions += 1;
    }
  }

  Object.defineProperty(globalThis, "Response", {
    ...descriptor,
    value: CountingResponse
  });
  try {
    const middleware = createMiddleware(testConfig());
    const response = await middleware(request, async () => new Response("ok"));
    assert.equal(response.status, 200);
    assert.equal(response.headers.get("x-request-id"), "single-finalizer");
    assert.equal(await response.text(), "ok");
    assert.equal(
      constructions,
      2,
      "one handler response plus exactly one finalization wrapper must be constructed"
    );
  } finally {
    Object.defineProperty(globalThis, "Response", descriptor);
  }

  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
});

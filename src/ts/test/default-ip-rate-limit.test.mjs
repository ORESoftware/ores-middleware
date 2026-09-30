import assert from "node:assert/strict";
import test from "node:test";

import { createMiddleware, defaultConfig } from "../dist/index.js";
import { attachTrustedPeerIp } from "../dist/client-ip.js";

function testConfig() {
  const config = defaultConfig("default-ip-rate-limit-test");
  config.environment = "test";
  config.settings.tls.mode = "disabled";
  config.settings.tls.requireHttps = false;
  config.settings.compression.enabled = false;
  config.settings.idempotency.enabled = false;
  config.settings.securityHeaders.enabled = false;
  return config;
}

test("default rate limit is five requests per second keyed only by IP", () => {
  const policy = defaultConfig("default-ip-rate-limit-test").settings.rateLimit;
  assert.equal(policy.enabled, true);
  assert.equal(policy.capacity, 5);
  assert.equal(policy.refillPerSecond, 5);
  assert.deepEqual(policy.keyBy, ["ip"]);
});

test("portable middleware keys the baseline strictly by resolved client IP", async () => {
  const keys = [];
  const middleware = createMiddleware(testConfig(), {
    clientIp: () => "203.0.113.9",
    rateLimiter: {
      async allow(key, capacity, refillPerSecond) {
        keys.push([key, capacity, refillPerSecond]);
        return true;
      }
    }
  });

  const response = await middleware(
    new Request("http://example.test/v1/other-route", {
      headers: { "x-real-ip": "198.51.100.77" }
    }),
    async () => new Response("ok")
  );

  assert.equal(response.status, 200);
  assert.deepEqual(keys, [["203.0.113.9", 5, 5]]);
});

test("strict mode rejects forwarded client identity from an untrusted peer", async () => {
  const keys = [];
  const middleware = createMiddleware(testConfig(), {
    isTrustedProxy: () => false,
    rateLimiter: {
      async allow(key) {
        keys.push(key);
        return true;
      }
    }
  });

  const response = await middleware(
    new Request("http://example.test/v1/items", {
      headers: {
        "x-real-ip": "203.0.113.1",
        "x-forwarded-for": "203.0.113.2"
      }
    }),
    async () => new Response("ok")
  );

  assert.equal(response.status, 400);
  assert.deepEqual(keys, []);
});

test("trusted proxy identity uses the first validated forwarded client IP", async () => {
  const keys = [];
  const middleware = createMiddleware(testConfig(), {
    isTrustedProxy: () => true,
    rateLimiter: {
      async allow(key) {
        keys.push(key);
        return true;
      }
    }
  });

  const response = await middleware(
    new Request("http://example.test/v1/items", {
      headers: { "x-forwarded-for": "203.0.113.40, 10.0.0.4" }
    }),
    async () => new Response("ok")
  );

  assert.equal(response.status, 200);
  assert.deepEqual(keys, ["203.0.113.40"]);
});


test("trusted forwarded client identity overrides the adapter-recorded proxy peer", async () => {
  const keys = [];
  const middleware = createMiddleware(testConfig(), {
    isTrustedProxy: () => true,
    rateLimiter: {
      async allow(key) {
        keys.push(key);
        return true;
      }
    }
  });

  const request = attachTrustedPeerIp(
    new Request("http://example.test/v1/items", {
      headers: { "x-forwarded-for": "203.0.113.55, 10.0.0.5" }
    }),
    "10.0.0.10"
  );
  const response = await middleware(request, async () => new Response("ok"));

  assert.equal(response.status, 200);
  assert.deepEqual(keys, ["203.0.113.55"]);
});

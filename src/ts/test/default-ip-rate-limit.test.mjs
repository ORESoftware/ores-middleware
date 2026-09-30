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
    new Request("http://example.test/v1/other-route"),
    async () => new Response("ok")
  );

  assert.equal(response.status, 200);
  assert.deepEqual(keys, [["203.0.113.9", 5, 5]]);
});

test("rate-limit denial exposes modern and compatibility backpressure metadata", async () => {
  const middleware = createMiddleware(testConfig(), {
    clientIp: () => "203.0.113.9",
    rateLimiter: { async allow() { return false; } }
  });

  const response = await middleware(
    new Request("http://example.test/v1/items"),
    async () => new Response("handler must not run")
  );

  assert.equal(response.status, 429);
  assert.equal(response.headers.get("retry-after"), "1");
  assert.equal(response.headers.get("ratelimit-policy"), "\"ip-default\";q=5;w=1");
  assert.equal(response.headers.get("ratelimit"), "\"ip-default\";r=0;t=1");
  assert.equal(response.headers.get("ratelimit-limit"), "5");
  assert.equal(response.headers.get("ratelimit-remaining"), "0");
  assert.equal(response.headers.get("ratelimit-reset"), "1");
  assert.equal(response.headers.get("x-ores-rate-limit-policy"), "ip-default");
  assert.equal(response.headers.get("x-ores-rate-limit-layer"), "application");
  assert.equal(response.headers.get("x-ores-rate-limit-decision"), "denied");
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

test("configured CIDRs trust adapter-attached loopback peers without a custom hook", async () => {
  const keys = [];
  const middleware = createMiddleware(testConfig(), {
    rateLimiter: {
      async allow(key) {
        keys.push(key);
        return true;
      }
    }
  });
  const request = attachTrustedPeerIp(
    new Request("http://example.test/v1/items", {
      headers: { "x-forwarded-for": "203.0.113.88" }
    }),
    "127.0.0.1"
  );
  const response = await middleware(request, async () => new Response("ok"));

  assert.equal(response.status, 200);
  assert.deepEqual(keys, ["203.0.113.88"]);
});

test("configured IPv6 CIDRs trust adapter-attached loopback peers", async () => {
  const keys = [];
  const middleware = createMiddleware(testConfig(), {
    rateLimiter: {
      async allow(key) {
        keys.push(key);
        return true;
      }
    }
  });
  const request = attachTrustedPeerIp(
    new Request("http://example.test/v1/items", {
      headers: { "x-forwarded-for": "2001:db8::44" }
    }),
    "::1"
  );
  const response = await middleware(request, async () => new Response("ok"));

  assert.equal(response.status, 200);
  assert.deepEqual(keys, ["2001:db8::44"]);
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


test("default local limiter bounds attacker-controlled IP cardinality", async () => {
  const config = testConfig();
  config.settings.rateLimit.capacity = 1;
  config.settings.rateLimit.refillPerSecond = 0.000001;
  let currentIp = "198.51.100.1";
  const middleware = createMiddleware(config, {
    clientIp: () => currentIp,
    now: () => 0
  });
  const next = async () => new Response("ok");

  assert.equal((await middleware(new Request("http://example.test/v1"), next)).status, 200);
  assert.equal((await middleware(new Request("http://example.test/v1"), next)).status, 429);

  for (let index = 0; index < 10_000; index += 1) {
    const high = Math.floor(index / 256);
    const low = index % 256;
    currentIp = `2001:db8:${high.toString(16)}::${low.toString(16)}`;
    assert.equal((await middleware(new Request("http://example.test/v1"), next)).status, 200);
  }

  currentIp = "198.51.100.1";
  assert.equal((await middleware(new Request("http://example.test/v1"), next)).status, 200);
});

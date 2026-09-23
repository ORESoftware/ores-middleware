import test from "node:test";
import assert from "node:assert/strict";

import {
  EdgeMinimalRequest,
  createEdgeMinimalCallbackArgs,
  defineEdgeMinimalMiddleware
} from "../dist/edge-provider-api.js";

const context = Object.freeze({
  requestId: "req-1",
  traceId: "trace-1",
  startedAtUnixMs: 1,
  baggage: {}
});

function dependencies() {
  return {
    fetch: {
      async fetch(request) {
        return {
          status: 204,
          headers: {},
          body: new TextEncoder().encode(request.url)
        };
      }
    },
    auth: {
      async verify() {
        return { userId: "user-1", claims: {} };
      }
    },
    rateLimiter: {
      async allow() {
        return true;
      }
    },
    cache: {
      async get() {
        return undefined;
      },
      async set() {},
      async delete() {}
    },
    telemetry: {
      started() {},
      finished() {}
    },
    log: {
      debug() {},
      info() {},
      warn() {},
      error() {}
    },
    now: () => 123,
    randomId: () => "id-1"
  };
}

test("edge_minimal injects approved dependencies directly and exposes only header mutation", async () => {
  const request = new EdgeMinimalRequest(
    new Request("https://example.test/v1/users?q=1", {
      method: "GET",
      headers: { authorization: "Bearer token" }
    }),
    {
      remoteIp: "127.0.0.1",
      contentLength: 0,
      transportSecure: true
    }
  );

  const middleware = defineEdgeMinimalMiddleware(async ({
    request,
    fetch,
    auth,
    rateLimiter,
    cache,
    log,
    now,
    randomId,
    setRequestHeader,
    continue: continueRequest
  }) => {
    assert.equal(request.method, "GET");
    assert.equal(request.path, "/v1/users?q=1");
    assert.equal(request.remoteIp, "127.0.0.1");
    assert.equal(request.transportSecure, true);
    assert.equal(now(), 123);
    assert.equal(randomId(), "id-1");

    const fetched = await fetch.fetch({ method: "GET", url: "https://auth.test/check" });
    assert.equal(fetched.status, 204);
    const identity = await auth.verify(request, context);
    assert.equal(identity.userId, "user-1");
    assert.equal(await rateLimiter.allow("user-1", 10, 1), true);
    await cache.set("seen:user-1", new Uint8Array([1]), 1000);
    log.info("edge middleware admitted request", { request_id: context.requestId });

    setRequestHeader("X-User-Id", identity.userId);
    return continueRequest();
  });

  const result = await middleware(createEdgeMinimalCallbackArgs(request, context, dependencies()));
  assert.equal(result.kind, "continue");
  assert.equal(result.request.header("x-user-id"), "user-1");
  assert.equal(result.request.method, "GET");
  assert.equal(result.request.path, "/v1/users?q=1");
});

test("edge_minimal header writes are canonicalized and reject framing bytes", () => {
  const request = new EdgeMinimalRequest(
    new Request("https://example.test/", { method: "GET" }),
    { transportSecure: true }
  );

  request.setHeader("X-ORES-Test", "ok");
  assert.equal(request.header("x-ores-test"), "ok");
  assert.throws(() => request.setHeader("bad header", "x"), /invalid middleware request header name/);
  assert.throws(() => request.setHeader("x-test", "bad\r\nvalue"), /invalid middleware request header value/);
});

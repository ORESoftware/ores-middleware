import assert from "node:assert/strict";
import test from "node:test";

import { createCloudflareEdgeMinimalHandler } from "../dist/providers/cloudflare.js";

const requestContext = {
  requestId: "request-1",
  traceId: "0123456789abcdef0123456789abcdef",
  startedAtUnixMs: 0,
  baggage: {}
};

const dependencies = {
  fetch: globalThis.fetch,
  auth: {
    async verify() {
      return {};
    }
  },
  rateLimit: {
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
    event() {}
  }
};

const executionContext = {
  waitUntil() {}
};

test("portable middleware cannot observe Cloudflare env directly", async () => {
  let observed = undefined;
  const middleware = async (args) => {
    observed = Object.keys(args).sort();
    args.request.setHeader("x-portable-middleware", "1");
    return { kind: "continue" };
  };

  const handler = createCloudflareEdgeMinimalHandler(middleware, {
    context() {
      return requestContext;
    },
    dependencies(_request, env) {
      assert.equal(env.KV, "provider-owned-binding");
      return dependencies;
    },
    origin(request, env) {
      assert.equal(env.KV, "provider-owned-binding");
      return new Response(request.headers.get("x-portable-middleware"));
    }
  });

  const response = await handler.fetch(
    new Request("https://example.invalid/test"),
    { KV: "provider-owned-binding" },
    executionContext
  );

  assert.equal(await response.text(), "1");
  assert.deepEqual(observed, [
    "auth",
    "cache",
    "context",
    "fetch",
    "rateLimit",
    "request",
    "telemetry"
  ]);
});

test("portable short-circuit does not invoke provider continuation", async () => {
  let originCalls = 0;
  const middleware = async () => {
    return {
      kind: "respond",
      response: new Response("blocked", { status: 403 })
    };
  };

  const handler = createCloudflareEdgeMinimalHandler(middleware, {
    context() {
      return requestContext;
    },
    dependencies() {
      return dependencies;
    },
    origin() {
      originCalls += 1;
      return new Response("origin");
    }
  });

  const response = await handler.fetch(
    new Request("https://example.invalid/test"),
    {},
    executionContext
  );

  assert.equal(response.status, 403);
  assert.equal(await response.text(), "blocked");
  assert.equal(originCalls, 0);
});

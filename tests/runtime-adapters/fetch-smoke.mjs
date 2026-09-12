import assert from "node:assert/strict";
import { createMiddleware, currentContext, defaultConfig } from "../../src/ts/dist/index.js";
import { fetchHandler } from "../../src/ts/dist/adapters.js";

const config = defaultConfig("native-fetch-conformance");
config.environment = "test";
config.settings.tls.mode = "disabled";
config.settings.tls.requireHttps = false;
config.settings.rateLimit.enabled = false;
config.settings.idempotency.enabled = false;
config.settings.compression.enabled = false;
config.settings.maxBodyBytes = 64;
let calls = 0;
const middleware = createMiddleware(config, {
  authVerifier: async request => ({ userId: request.headers.get("x-test-user") ?? "local-test", tenantId: "test-only" }),
  operationFailureReporter() {},
  requestContractValidator: {
    resolve(method, pathname) {
      if (method !== "POST" || pathname !== "/echo") return undefined;
      return { pathTemplate: "/echo", async validate(input) {
        const value = await input.body.json();
        return value && typeof value === "object" && typeof value.value === "string"
          ? [] : [{ path: "/body/value", code: "type", message: "expected string" }];
      } };
    }
  }
});
const handle = fetchHandler(middleware, async (request, context) => {
  calls++;
  await new Promise(resolve => setTimeout(resolve, 1));
  assert.equal(currentContext()?.userId, request.headers.get("x-test-user") ?? "local-test");
  if (context?.params) assert.equal(await context.params, "preserved");
  return Response.json(await request.json(), { status: 201 });
});
function request(url, value = "ok", user = "local-test") {
  return new Request(url, { method: "POST", headers: { "content-type": "application/json", "x-test-user": user }, body: JSON.stringify({ value }) });
}
const context = { params: Promise.resolve("preserved") };
const result = await handle(request("http://127.0.0.1/echo"), context);
assert.equal(result.status, 201); assert.deepEqual(await result.json(), { value: "ok" });
assert.equal((await handle(request("http://127.0.0.1/unknown"))).status, 404);
assert.equal((await handle(request("http://127.0.0.1/echo", 42))).status, 400);
assert.equal((await handle(request("http://127.0.0.1/echo", "x".repeat(128)))).status, 413);
assert.equal(calls, 1);
await Promise.all(Array.from({ length: 16 }, (_, i) => handle(request("http://127.0.0.1/echo", "ok", `user-${i}`))));
assert.equal(currentContext(), undefined);
let liveTransport = false;
if (globalThis.Bun) {
  const server = Bun.serve({ port: 0, hostname: "127.0.0.1", fetch: handle });
  try {
    assert.equal((await fetch(request(`http://127.0.0.1:${server.port}/echo`))).status, 201);
    liveTransport = true;
  } finally { await server.stop(true); }
} else if (globalThis.Deno) {
  const server = Deno.serve({ port: 0, hostname: "127.0.0.1", onListen() {} }, handle);
  try {
    assert.equal((await fetch(request(`http://127.0.0.1:${server.addr.port}/echo`))).status, 201);
    liveTransport = true;
  } finally { await server.shutdown(); }
}
console.log(JSON.stringify({ schema: "ores.middleware.native-fetch-smoke/v1", status: "passed", runtime: globalThis.Bun?.version ?? globalThis.Deno?.version.deno ?? process.version, liveTransport, concurrentRequests: 16, rejectionCases: 3 }));

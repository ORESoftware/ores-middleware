import assert from "node:assert/strict";
import test from "node:test";
import { createMiddleware, defaultConfig } from "../dist/index.js";

function middleware(service, dependencies = {}) {
  const config = defaultConfig(service);
  config.environment = "test";
  config.settings.tls.requireHttps = false;
  config.settings.rateLimit.enabled = false;
  config.settings.compression.enabled = false;
  config.settings.maxBodyBytes = 16;
  config.settings.timeoutMs = 40;
  return createMiddleware(config, { operationFailureReporter: () => {}, ...dependencies });
}
test("installed core rejects unadvertised and understated bodies before validators or handlers", async () => {
  let validated = 0; let dispatched = 0;
  const run = middleware("limits", { requestContractValidator: { resolve() { validated++; return undefined; } } });
  for (const headers of [{}, { "content-length": "1" }]) {
    const response = await run(new Request("https://example.test/orders", { method: "POST", headers, body: "x".repeat(17) }), async () => { dispatched++; return new Response(); });
    assert.equal(response.status, 413);
    assert.equal(response.headers.has("x-request-id"), true);
  }
  assert.equal(validated, 0); assert.equal(dispatched, 0);
});
test("installed core terminates a stalled body without dispatching the handler", async () => {
  let dispatched = false;
  const body = new ReadableStream({ pull() { return new Promise(() => {}); }, cancel() { return new Promise(() => {}); } });
  const response = await middleware("deadlines")(new Request("https://example.test/orders", { method: "POST", body, duplex: "half" }), async () => { dispatched = true; return new Response(); });
  assert.equal(response.status, 504); assert.equal(dispatched, false);
});
test("installed TypeScript stack isolates authenticated replay scope and correlation", async () => {
  const entries = new Map(); let calls = 0;
  const dependencies = {
    // Synthetic test-only identities, not a production authentication adapter.
    authVerifier: async request => ({ tenantId: request.headers.get("x-test-tenant"), userId: request.headers.get("x-test-user") }),
    idempotencyStore: { get: async key => entries.get(key), set: async (key, value) => { entries.set(key, value); } }
  };
  const handlers = { orders: middleware("orders", dependencies), admin: middleware("admin", dependencies) };
  const invoke = (service, tenant, user, target, requestId) => handlers[service](new Request(target, {
    method: "POST", body: "{}", headers: { "idempotency-key": "same-key", "x-test-tenant": tenant, "x-test-user": user, "x-request-id": requestId }
  }), async () => new Response(String(++calls), { status: 201 }));
  const first = await invoke("orders", "a", "a", "https://example.test/orders?x=1", "first");
  const replay = await invoke("orders", "a", "a", "https://example.test/orders?x=1", "replay");
  assert.equal(await first.text(), "1"); assert.equal(await replay.text(), "1");
  assert.equal(calls, 1); assert.equal(replay.headers.get("x-request-id"), "replay");
  for (const [service, tenant, user, target] of [
    ["orders", "b", "a", "https://example.test/orders?x=1"],
    ["orders", "a", "b", "https://example.test/orders?x=1"],
    ["orders", "a", "a", "https://example.test/orders?x=2"],
    ["orders", "a", "a", "https://example.test/orders/other?x=1"],
    ["admin", "a", "a", "https://example.test/orders?x=1"]
  ]) {
    const response = await invoke(service, tenant, user, target, `scope-${calls}`);
    assert.equal(response.status, 201); assert.notEqual(await response.text(), "1");
  }
  assert.equal(calls, 6);
  assert.ok([...entries.keys()].every(key => /^ores:idempotency:v2:[0-9a-f]{64}$/.test(key)));
});

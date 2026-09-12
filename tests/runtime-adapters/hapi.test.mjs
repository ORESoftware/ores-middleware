import assert from "node:assert/strict";
import test from "node:test";
import { createRequire } from "node:module";
import { hapiHandler, hapiLifecycle } from "../../src/ts/dist/adapters.js";
import { createMiddleware, currentContext, defaultConfig } from "../../src/ts/dist/index.js";

// CI installs the exact Hapi version in an isolated, non-shipping test prefix.
const require = createRequire(new URL("../../target/runtime-adapters/package.json", import.meta.url));
const Hapi = require("@hapi/hapi");
function config() {
  const value = defaultConfig("hapi-adapter-conformance");
  value.environment = "test";
  value.settings.tls.requireHttps = false;
  value.settings.tls.mode = "disabled";
  value.settings.rateLimit.enabled = false;
  value.settings.idempotency.enabled = false;
  value.settings.compression.enabled = false;
  return value;
}
function server(t) {
  const value = Hapi.server({ debug: false });
  t.after(() => value.stop());
  return value;
}

test("actual Hapi onPreHandler does not route a middleware 204", async t => {
  const app = server(t); let calls = 0;
  app.ext("onPreHandler", hapiLifecycle(async () => new Response(null, { status: 204 })));
  app.route({ method: "GET", path: "/", handler() { calls++; return "must not run"; } });
  const result = await app.inject("/");
  assert.equal(result.statusCode, 204); assert.equal(calls, 0);
});
test("actual Hapi denial uses supported headers and separate cookies", async t => {
  const app = server(t);
  const headers = new Headers({ "x-policy": "denied" });
  headers.append("set-cookie", "a=1; Expires=Wed, 21 Oct 2030 07:28:00 GMT");
  headers.append("set-cookie", "b=2; HttpOnly");
  app.ext("onPreHandler", hapiLifecycle(async () => new Response("denied", { status: 403, headers })));
  app.route({ method: "GET", path: "/", handler: () => "must not run" });
  const result = await app.inject("/");
  assert.equal(result.statusCode, 403); assert.equal(result.payload, "denied");
  assert.equal(result.headers["x-policy"], "denied");
  assert.deepEqual(result.headers["set-cookie"], headers.getSetCookie());
});
for (const payload of [false, 0, "hello", { value: "ok" }]) {
  test(`actual Hapi parsed JSON round trip: ${JSON.stringify(payload)}`, async t => {
    const app = server(t); const finished = [];
    const middleware = createMiddleware(config(), {
      authVerifier: async () => ({ userId: "user-a", tenantId: "tenant-a" }),
      telemetry: { started() {}, finished(context, request, response) { finished.push(response.status); } }
    });
    app.route({
      method: "POST", path: "/", options: { payload: { maxBytes: 1024 } },
      handler: hapiHandler(middleware, async request => {
        await Promise.resolve(); assert.equal(currentContext()?.tenantId, "tenant-a");
        return Response.json(await request.json(), { status: 201 });
      })
    });
    const result = await app.inject({ method: "POST", url: "/", headers: { "content-type": "application/json" }, payload: JSON.stringify(payload) });
    assert.equal(result.statusCode, 201, result.payload);
    assert.deepEqual(JSON.parse(result.payload), payload);
    assert.deepEqual(finished, [201]); assert.equal(currentContext(), undefined);
    assert.ok(result.headers["x-request-id"]);
  });
}
test("actual Hapi raw bytes stay intact and oversized input stops before the handler", async t => {
  const app = server(t); let calls = 0;
  app.route({ method: "POST", path: "/", options: { payload: { parse: false, output: "data", maxBytes: 8 } },
    handler: hapiHandler((request, next) => next(request), async request => {
      calls++; return new Response(await request.arrayBuffer());
    })
  });
  let result = await app.inject({ method: "POST", url: "/", payload: Buffer.from([0, 1, 2, 255]) });
  assert.equal(result.statusCode, 200); assert.deepEqual(result.rawPayload, Buffer.from([0, 1, 2, 255]));
  result = await app.inject({ method: "POST", url: "/", payload: Buffer.alloc(9) });
  assert.equal(result.statusCode, 413); assert.equal(calls, 1);
});
test("actual Hapi handler failures are mapped by portable middleware", async t => {
  const app = server(t);
  app.route({ method: "GET", path: "/", handler: hapiHandler(createMiddleware(config(), { operationFailureReporter() {} }), () => {
    throw new Error("do-not-disclose");
  }) });
  const result = await app.inject("/");
  assert.equal(result.statusCode, 500); assert.ok(!result.payload.includes("do-not-disclose"));
});

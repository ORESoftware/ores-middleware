import assert from "node:assert/strict";
import test from "node:test";
import { AsyncLocalStorage } from "node:async_hooks";
import { fetchHandler } from "../dist/fetch-adapter.js";
import { hapiHandler, hapiLifecycle } from "../dist/hapi.js";

const pass = (request, next) => next(request);
const native = (payload, method = "post", headers = {}) => ({
  url: new URL("https://example.test/orders"), method, headers, payload
});
function toolkit() {
  return {
    continue: Symbol("continue"),
    response(body) {
      return {
        body, statusCode: 200, headers: new Map(), takenOver: false,
        code(status) { this.statusCode = status; return this; },
        header(name, value, options = {}) {
          const existing = this.headers.get(name) ?? [];
          this.headers.set(name, options.append ? [...existing, value] : [value]);
          return this;
        },
        takeover() { this.takenOver = true; return this; }
      };
    }
  };
}

test("Hapi short-circuit 204 is a takeover, never admission", async () => {
  const h = toolkit();
  const result = await hapiLifecycle(async () => new Response(null, { status: 204 }))(native(null, "get"), h);
  assert.notEqual(result, h.continue);
  assert.equal(result.statusCode, 204);
  assert.equal(result.takenOver, true);
});
test("Hapi continuation requires calling the downstream admission callback", async () => {
  const h = toolkit();
  assert.equal(await hapiLifecycle(pass)(native(null, "get"), h), h.continue);
});
test("Hapi post-admission rejection takes over", async () => {
  const h = toolkit();
  const result = await hapiLifecycle(async (request, next) => {
    await next(request); return new Response("denied", { status: 403 });
  })(native(null, "get"), h);
  assert.equal(result.statusCode, 403);
  assert.equal(result.takenOver, true);
});
test("Hapi uses header() and preserves independent cookies", async () => {
  const headers = new Headers({ "x-policy": "enforced" });
  headers.append("set-cookie", "a=1; Expires=Wed, 21 Oct 2030 07:28:00 GMT");
  headers.append("set-cookie", "b=2; HttpOnly");
  const result = await hapiLifecycle(async () => new Response("no", { status: 401, headers }))(native(null, "get"), toolkit());
  assert.equal(result.statusCode, 401);
  assert.deepEqual(result.headers.get("set-cookie"), headers.getSetCookie());
  assert.deepEqual(result.headers.get("x-policy"), ["enforced"]);
});
for (const [label, payload, expected, headers] of [
  ["false", false, "false", {}],
  ["zero", 0, "0", {}],
  ["empty string", "", "", {}],
  ["plain text", "hello", "hello", {}],
  ["bytes", Buffer.from("raw"), "raw", {}],
  ["JSON", { count: 2 }, '{"count":2}', {}],
  ["framed null", null, "null", { "content-length": "4" }]
]) {
  test(`Hapi preserves ${label} payload`, async () => {
    let observed;
    await hapiHandler(pass, async request => {
      observed = await request.text(); return new Response(null, { status: 204 });
    })(native(payload, "post", headers), toolkit());
    assert.equal(observed, expected);
  });
}
test("Hapi absent GET/HEAD payload does not become a Fetch body", async () => {
  for (const method of ["get", "head"]) {
    await hapiHandler(pass, request => {
      assert.equal(request.body, null); return new Response(null, { status: 204 });
    })(native(null, method), toolkit());
  }
});
test("Hapi cannot accidentally stringify a payload stream", async () => {
  await assert.rejects(hapiHandler(pass, () => new Response())(
    native({ pipe() {} }), toolkit()
  ), /streaming adapter/);
});
test("Hapi reconstruction removes stale framing and compression headers", async () => {
  await hapiHandler(pass, request => {
    for (const name of ["content-length", "transfer-encoding", "content-encoding"]) {
      assert.equal(request.headers.has(name), false);
    }
    return new Response(null, { status: 204 });
  })(native("decoded", "post", { "content-length": "999", "content-encoding": "gzip" }), toolkit());
});
test("Hapi middleware observes real response and waits for handler in its scope", async () => {
  const scope = new AsyncLocalStorage();
  const events = [];
  const middleware = (request, next) => scope.run("request-a", async () => {
    events.push("before"); const response = await next(request);
    assert.equal(response.status, 201); events.push("after"); return response;
  });
  const h = toolkit(); const req = native("body");
  const result = await hapiHandler(middleware, async (request, nativeRequest, toolkit_) => {
    await Promise.resolve();
    assert.equal(scope.getStore(), "request-a");
    assert.equal(nativeRequest, req); assert.equal(toolkit_, h);
    assert.equal(await request.text(), "body"); events.push("handler");
    return new Response("created", { status: 201 });
  })(req, h);
  assert.equal(result.statusCode, 201);
  assert.deepEqual(events, ["before", "handler", "after"]);
  assert.equal(scope.getStore(), undefined);
});
test("Hapi request-scoped replacement reaches the actual route", async () => {
  const middleware = (request, next) => next(new Request(request, { headers: { "x-validated": "yes" } }));
  await hapiHandler(middleware, request => {
    assert.equal(request.headers.get("x-validated"), "yes"); return new Response();
  })(native(undefined, "get"), toolkit());
});
test("Hapi wrapper never dispatches a rejected request", async () => {
  let calls = 0;
  const result = await hapiHandler(async () => new Response("no", { status: 401 }), () => {
    calls++; return new Response();
  })(native(null, "get"), toolkit());
  assert.equal(calls, 0); assert.equal(result.statusCode, 401);
});
test("Hapi route errors stay inside the middleware boundary", async () => {
  const middleware = async (request, next) => {
    try { return await next(request); } catch { return new Response("safe", { status: 500 }); }
  };
  const result = await hapiHandler(middleware, () => { throw new Error("private"); })(native(null, "get"), toolkit());
  assert.equal(result.statusCode, 500); assert.equal(String(result.body), "safe");
});
test("Fetch preserves Next.js/Deno/Bun context arguments and scoped Request", async () => {
  const params = Promise.resolve({ id: "a" }); const context = { params }; const server = {};
  const result = await fetchHandler((request, next) => next(new Request(request, { headers: { "x-validated": "yes" } })), async (request, context_, server_) => {
    assert.equal(request.headers.get("x-validated"), "yes");
    assert.equal(context_, context); assert.equal(server_, server);
    assert.equal(context_.params, params); return new Response((await params).id);
  })(new Request("https://example.test"), context, server);
  assert.equal(await result.text(), "a");
});
test("Fetch request context remains isolated across concurrent handlers", async () => {
  const scope = new AsyncLocalStorage();
  const handle = fetchHandler((request, next) => scope.run(request.url, () => next(request)), async request => {
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(scope.getStore(), request.url); return new Response();
  });
  await Promise.all(Array.from({ length: 32 }, (_, i) => handle(new Request(`https://example.test/${i}`))));
  assert.equal(scope.getStore(), undefined);
});
for (const [label, wrap] of [["Fetch", handler => fetchHandler(handler, () => new Response())], ["Hapi", handler => {
  const wrapped = hapiHandler(handler, () => new Response());
  return () => wrapped(native(null, "get"), toolkit());
}]]) {
  test(`${label} refuses double dispatch`, async () => {
    const handle = wrap(async (request, next) => { await next(request); return next(request); });
    await assert.rejects(handle(new Request("https://example.test")), /twice/);
  });
}
test("Fetch rejects non-Response handler results", async () => {
  await assert.rejects(fetchHandler(pass, () => undefined)(new Request("https://example.test")), /Response/);
});

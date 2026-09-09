import assert from "node:assert/strict";
import test from "node:test";
import { hapiHandler } from "../dist/hapi.js";
const pass = (request, next) => next(request);
const native = (payload, method, headers) => ({ url: new URL("https://example.test/orders"), method, headers, payload });
const toolkit = () => ({ response(body) { return { body, code() { return this; }, header() { return this; } }; } });

test("Hapi re-encodes parsed JSON strings but leaves unparsed strings alone", async () => {
  for (const parse of [true, false]) {
    const req = native("hello", "post", { "content-type": "application/json" });
    req.route = { settings: { payload: { parse } } };
    await hapiHandler(pass, async request => {
      assert.equal(await request.text(), parse ? '\"hello\"' : 'hello');
      return new Response();
    })(req, toolkit());
  }
});
test("Hapi preserves parsed structured JSON media-type strings", async () => {
  await hapiHandler(pass, async request => {
    assert.equal(await request.json(), "hello"); return new Response();
  })(native("hello", "post", { "content-type": "application/example+json; charset=utf-8" }), toolkit());
});

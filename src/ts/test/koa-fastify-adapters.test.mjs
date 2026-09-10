import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import test from "node:test";

import { createMiddleware, defaultConfig } from "../dist/index.js";
import { koaMiddleware } from "../dist/koa.js";
import { fastifyPreHandler } from "../dist/fastify.js";

function config(serviceName = "framework-adapter-test") {
  const value = defaultConfig(serviceName);
  value.settings.rateLimit.enabled = false;
  value.settings.idempotency.enabled = false;
  value.settings.compression.enabled = false;
  return value;
}

test("Koa adapter keeps portable context active through downstream middleware", async () => {
  let observedContext;
  const middleware = createMiddleware(config());
  const responseHeaders = {};
  const context = {
    method: "GET",
    protocol: "https",
    host: "example.test",
    originalUrl: "/koa",
    request: { headers: { accept: "application/json", "x-request-id": "koa-request" } },
    response: { headers: responseHeaders },
    state: {},
    status: 404,
    body: null,
    set(name, value) { responseHeaders[name.toLowerCase()] = value; }
  };

  await koaMiddleware(middleware)(context, async () => {
    observedContext = context.state.oresContext;
    context.status = 201;
    context.response.headers["x-handler"] = "koa";
    context.body = { ok: true };
  });

  assert.equal(observedContext.requestId, "koa-request");
  assert.equal(context.status, 201);
  assert.equal(context.response.headers["x-handler"], "koa");
  assert.equal(context.response.headers["x-request-id"], "koa-request");
  assert.equal(JSON.parse(context.body.toString()).ok, true);
});

test("Koa adapter propagates portable short-circuit responses", async () => {
  const value = config();
  value.settings.tls.requireHttps = true;
  const middleware = createMiddleware(value);
  let downstream = 0;
  const responseHeaders = {};
  const context = {
    method: "GET",
    protocol: "http",
    host: "example.test",
    originalUrl: "/koa-denied",
    request: { headers: {} },
    response: { headers: responseHeaders },
    state: {},
    set(name, headerValue) { responseHeaders[name.toLowerCase()] = headerValue; }
  };

  await koaMiddleware(middleware)(context, async () => { downstream += 1; });
  assert.equal(downstream, 0);
  assert.equal(context.status, 426);
  assert.match(context.body.toString(), /https_required/);
});

function fastifyHarness({ protocol = "https", url = "/fastify", headers = {} } = {}) {
  const raw = new EventEmitter();
  raw.url = url;
  raw.method = "GET";
  raw.headers = { host: "example.test", ...headers };
  raw.statusCode = 200;
  raw.finished = false;
  raw.writableEnded = false;
  raw.responseHeaders = {};
  raw.getHeaders = () => ({ ...raw.responseHeaders });
  raw.setHeader = (name, value) => { raw.responseHeaders[name.toLowerCase()] = value; };

  const request = { raw, method: "GET", protocol, hostname: "example.test", url, headers: raw.headers };
  const reply = {
    raw,
    sent: false,
    statusCode: 200,
    payload: undefined,
    code(status) { this.statusCode = status; raw.statusCode = status; return this; },
    header(name, value) { raw.setHeader(name, value); return this; },
    send(payload) {
      this.sent = true;
      this.payload = payload;
      raw.finished = true;
      raw.writableEnded = true;
      raw.emit("finish");
      return this;
    }
  };
  return { request, reply, raw };
}

test("Fastify adapter releases the hook and finalizes after the native response", async () => {
  let finished;
  const finishedPromise = new Promise((resolve) => { finished = resolve; });
  const middleware = createMiddleware(config(), {
    telemetry: {
      started() {},
      finished(context, _request, response) { finished({ context, response }); }
    }
  });
  const { request, reply, raw } = fastifyHarness({ headers: { "x-request-id": "fastify-request" } });
  let doneCalls = 0;
  let doneError;

  fastifyPreHandler(middleware)(request, reply, (error) => {
    doneCalls += 1;
    doneError = error;
    queueMicrotask(() => {
      reply.code(202).header("x-handler", "fastify");
      raw.finished = true;
      raw.writableEnded = true;
      raw.emit("finish");
    });
  });

  const result = await finishedPromise;
  assert.equal(doneCalls, 1);
  assert.equal(doneError, undefined);
  assert.equal(request.oresContext.requestId, "fastify-request");
  assert.equal(result.context.requestId, "fastify-request");
  assert.equal(result.response.status, 202);
  assert.equal(result.response.headers.get("x-handler"), "fastify");
  assert.equal(reply.sent, false);
});

test("Fastify adapter sends portable short-circuit responses without continuing", async () => {
  const value = config();
  value.settings.tls.requireHttps = true;
  const middleware = createMiddleware(value);
  const { request, reply } = fastifyHarness({ protocol: "http", url: "/fastify-denied" });
  let doneCalls = 0;

  fastifyPreHandler(middleware)(request, reply, () => { doneCalls += 1; });
  for (let i = 0; i < 20 && !reply.sent; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }

  assert.equal(doneCalls, 0);
  assert.equal(reply.sent, true);
  assert.equal(reply.statusCode, 426);
  assert.match(reply.payload.toString(), /https_required/);
});

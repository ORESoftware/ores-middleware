import { EventEmitter } from "node:events";
import test from "node:test";
import assert from "node:assert/strict";

import { currentContext, defaultConfig } from "../dist/index.js";
import {
  currentLoggedInUserId,
  currentRequestId,
  currentTenantId,
  currentTraceId
} from "../dist/context.js";
import {
  createNestjsMiddleware,
  expressMiddleware,
  nestjsMiddleware,
  nodeRequestContext,
  nodeRequestLogger
} from "../dist/adapters.js";
import {
  createLogger,
  createOresOtelMiddleware,
  getLogContext
} from "../dist/otel.js";

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

function testConfig(timeoutMs = 5_000) {
  const config = defaultConfig("node-adapter-test");
  config.environment = "test";
  config.settings.timeoutMs = timeoutMs;
  config.settings.tls.mode = "disabled";
  config.settings.tls.requireHttps = false;
  config.settings.rateLimit.enabled = false;
  config.settings.idempotency.enabled = false;
  config.settings.compression.enabled = false;
  return config;
}

function memoryLoggers(records) {
  const transport = {
    name: "memory",
    write(record) {
      records.push(record);
    }
  };
  return {
    root: createLogger({
      appName: "node-adapter-test",
      name: "server",
      console: false,
      transports: transport
    }),
    fileLogger: createLogger({
      appName: "node-adapter-test",
      name: "route",
      console: false,
      transports: transport
    })
  };
}

class MockResponse extends EventEmitter {
  statusCode = 200;
  headersSent = false;
  writableEnded = false;
  finished = false;
  chunks = [];
  #headers = new Map();

  status(code) {
    this.statusCode = code;
    return this;
  }

  setHeader(name, value) {
    this.#headers.set(String(name).toLowerCase(), value);
    return this;
  }

  getHeader(name) {
    return this.#headers.get(String(name).toLowerCase());
  }

  getHeaders() {
    return Object.fromEntries(this.#headers);
  }

  end(body) {
    if (this.writableEnded) return this;
    if (body !== undefined) this.chunks.push(Buffer.from(body));
    this.headersSent = true;
    this.writableEnded = true;
    this.finished = true;
    queueMicrotask(() => this.emit("finish"));
    return this;
  }

  send(body) {
    return this.end(body);
  }

  bodyText() {
    return Buffer.concat(this.chunks).toString("utf8");
  }
}

function nodeRequest(slot) {
  return {
    method: "GET",
    protocol: "http",
    originalUrl: `/orders/${slot}`,
    headers: {
      host: "example.test",
      "x-request-id": `request-${slot}`,
      "x-test-user": `user-${slot}`,
      "x-test-tenant": `tenant-${slot}`,
      traceparent: `00-${Number(slot + 1).toString(16).padStart(32, "0")}-0123456789abcdef-01`
    },
    socket: { encrypted: false }
  };
}

function runNodeAdapter(adapter, request, downstream) {
  const response = new MockResponse();
  let nextCalls = 0;

  const completed = new Promise((resolve, reject) => {
    response.once("finish", () => resolve({ request, response, nextCalls }));
    adapter(request, response, (error) => {
      nextCalls += 1;
      // The regression used `next(callback)`, which Express interprets as an error.
      if (error !== undefined) {
        reject(error);
        return;
      }
      if (nextCalls > 1) {
        reject(new Error("next() called more than once"));
        return;
      }
      Promise.resolve(downstream(request, response)).catch(reject);
    });
  });

  return completed;
}

test("Express keeps ALS and request-attached context isolated until each response finishes", async () => {
  const records = [];
  const { root, fileLogger } = memoryLoggers(records);
  const middleware = createOresOtelMiddleware(testConfig(), {
    logger: root,
    authVerifier: async (request) => ({
      userId: request.headers.get("x-test-user"),
      tenantId: request.headers.get("x-test-tenant")
    })
  });
  const adapter = expressMiddleware(middleware);
  const count = 32;

  const results = await Promise.all(
    Array.from({ length: count }, (_, slot) => {
      const request = nodeRequest(slot);
      return runNodeAdapter(adapter, request, async (nativeRequest, response) => {
        await delay((count - slot) % 6);

        const context = currentContext();
        assert.equal(context?.requestId, `request-${slot}`);
        assert.equal(currentRequestId(), `request-${slot}`);
        assert.equal(currentLoggedInUserId(), `user-${slot}`);
        assert.equal(currentTenantId(), `tenant-${slot}`);
        assert.equal(currentTraceId(), Number(slot + 1).toString(16).padStart(32, "0"));
        assert.equal(getLogContext()?.fields?.["request.id"], `request-${slot}`);

        const attached = nodeRequestContext(nativeRequest);
        assert.equal(attached?.requestId, `request-${slot}`);
        assert.equal(attached?.userId, `user-${slot}`);
        assert.equal(Object.isFrozen(attached), true);
        assert.equal(Object.isFrozen(attached?.baggage), true);

        const requestLog = nodeRequestLogger(nativeRequest);
        assert.ok(requestLog);
        assert.equal(nativeRequest.log, requestLog);
        await fileLogger.info(`express-file:${slot}`).send();
        await requestLog.warn(`express-request:${slot}`).send();
        response.status(204).end();
      });
    })
  );

  await delay(40);
  assert.equal(results.length, count);
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);

  for (let slot = 0; slot < count; slot += 1) {
    const result = results[slot];
    assert.equal(result.nextCalls, 1);
    assert.equal(result.response.statusCode, 204);
    assert.equal(result.response.getHeader("x-request-id"), `request-${slot}`);
    assert.equal(
      result.response.getHeader("traceparent"),
      undefined,
      "the inbound parent span must not be relabelled as a server span"
    );

    for (const message of [`express-file:${slot}`, `express-request:${slot}`]) {
      const matching = records.filter((record) => record.message === message);
      assert.equal(matching.length, 1, `expected one ${message} record`);
      assert.equal(matching[0].fields["request.id"], `request-${slot}`);
      assert.equal(matching[0].fields["user.id"], `user-${slot}`);
      assert.equal(matching[0].fields["tenant.id"], `tenant-${slot}`);
    }
  }
});

test("sealed native requests use WeakMap context and logger carriers", async () => {
  const records = [];
  const { root } = memoryLoggers(records);
  const middleware = createOresOtelMiddleware(testConfig(), {
    logger: root,
    authVerifier: async () => ({ userId: "sealed-user", tenantId: "sealed-tenant" })
  });
  const request = nodeRequest(99);
  Object.preventExtensions(request);

  await runNodeAdapter(expressMiddleware(middleware), request, async (nativeRequest, response) => {
    assert.equal(nativeRequest.oresContext, undefined);
    assert.equal(nativeRequest.log, undefined);
    assert.equal(nodeRequestContext(nativeRequest)?.userId, "sealed-user");
    assert.ok(nodeRequestLogger(nativeRequest));
    response.status(204).end();
  });

  await delay(20);
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
});

test("NestJS global middleware uses the same correct native lifecycle bridge", async () => {
  assert.equal(nestjsMiddleware, createNestjsMiddleware);
  const records = [];
  const { root } = memoryLoggers(records);
  const middleware = createOresOtelMiddleware(testConfig(), {
    logger: root,
    authVerifier: async () => ({ userId: "nest-user", tenantId: "nest-tenant" })
  });
  const request = nodeRequest(7);

  const result = await runNodeAdapter(
    nestjsMiddleware(middleware),
    request,
    async (nativeRequest, response) => {
      await Promise.resolve();
      assert.equal(currentLoggedInUserId(), "nest-user");
      assert.equal(nodeRequestContext(nativeRequest)?.tenantId, "nest-tenant");
      assert.ok(nodeRequestLogger(nativeRequest));
      response.status(202).end();
    }
  );

  await delay(20);
  assert.equal(result.nextCalls, 1);
  assert.equal(result.response.statusCode, 202);
  assert.equal(currentContext(), undefined);
});

test("middleware short-circuits are written without calling Express next", async () => {
  const records = [];
  const { root } = memoryLoggers(records);
  const config = testConfig();
  config.integrations.sharedAuth.mode = "embedded";
  const middleware = createOresOtelMiddleware(config, { logger: root });
  const request = nodeRequest(5);
  const response = new MockResponse();
  let nextCalls = 0;

  const finished = new Promise((resolve, reject) => {
    response.once("finish", resolve);
    expressMiddleware(middleware)(request, response, (error) => {
      nextCalls += 1;
      reject(error ?? new Error("next must not run for an authentication short-circuit"));
    });
  });

  await finished;
  assert.equal(nextCalls, 0);
  assert.equal(response.statusCode, 401);
  assert.match(response.bodyText(), /authentication_required/);
  assert.equal(currentContext(), undefined);
});

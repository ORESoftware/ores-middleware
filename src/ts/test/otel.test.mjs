import test from "node:test";
import assert from "node:assert/strict";

import { defaultConfig } from "../dist/index.js";
import {
  createLogger,
  createOresOtelMiddleware,
  getLogContext,
  requestLogger
} from "../dist/otel.js";

test("ores-otel middleware pins file and request loggers to authenticated request context", async () => {
  const records = [];
  const transport = {
    name: "memory",
    write(record) {
      records.push(record);
    }
  };
  const root = createLogger({ appName: "middleware-test", console: false, transports: transport });
  const fileLogger = createLogger({ appName: "middleware-test", name: "handler", console: false, transports: transport });
  const config = defaultConfig("middleware-test");
  config.settings.tls.mode = "disabled";
  config.settings.tls.requireHttps = false;
  config.settings.rateLimit.enabled = false;
  config.settings.idempotency.enabled = false;
  config.settings.compression.enabled = false;

  const middleware = createOresOtelMiddleware(config, {
    logger: root,
    authVerifier: async () => ({ userId: "user-42", tenantId: "tenant-7" })
  });

  const response = await middleware(
    new Request("http://example.test/orders/42", {
      headers: {
        "x-request-id": "request-42",
        traceparent: "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01"
      }
    }),
    async (request) => {
      assert.equal(request.log, requestLogger(request));
      assert.equal(getLogContext()?.fields?.["request.id"], "request-42");
      assert.equal(getLogContext()?.fields?.["user.id"], "user-42");
      assert.equal(getLogContext()?.fields?.["tenant.id"], "tenant-7");
      await fileLogger.info("handler reached").send();
      await request.log.warn("slow dependency").send();
      return new Response("ok", { status: 202 });
    }
  );

  assert.equal(response.status, 202);
  assert.ok(records.some((record) => record.message === "request handler started"));
  assert.ok(records.some((record) => record.message === "request handler completed" && record.fields["http.response.status_code"] === 202));
  assert.ok(records.some((record) => record.message === "handler reached" && record.fields["request.id"] === "request-42"));
  assert.ok(records.some((record) => record.message === "slow dependency" && record.loggedInUser?.id === "user-42"));
  assert.equal(getLogContext(), undefined);
});

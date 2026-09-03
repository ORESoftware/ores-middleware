import test from "node:test";
import assert from "node:assert/strict";

import {
  createMiddleware,
  currentContext,
  defaultConfig
} from "../dist/index.js";
import { getLogContext } from "@oresoftware/next-loggers/context";

function testConfig() {
  const config = defaultConfig("http-lifecycle-boundary-test");
  config.settings.tls.mode = "disabled";
  config.settings.tls.requireHttps = false;
  config.settings.rateLimit.enabled = false;
  config.settings.compression.enabled = false;
  config.settings.idempotency.enabled = false;
  return config;
}

function request(requestId) {
  return new Request("http://example.test/profile", {
    headers: {
      accept: "application/json",
      "x-request-id": requestId,
      traceparent: `00-${requestId.padEnd(32, "0").slice(0, 32)}-0123456789abcdef-01`
    }
  });
}

function recorder(reports) {
  return async ({ failure }) => {
    reports.push({
      failure,
      requestId: currentContext()?.requestId,
      userId: currentContext()?.userId,
      logRequestId: getLogContext()?.fields?.["request.id"],
      logUserId: getLogContext()?.fields?.["user.id"]
    });
  };
}

test("authentication exceptions are contained inside the base request scope", async () => {
  const reports = [];
  const middleware = createMiddleware(testConfig(), {
    operationFailureReporter: recorder(reports),
    authVerifier: async (_request, context) => {
      assert.equal(currentContext()?.requestId, context.requestId);
      assert.equal(getLogContext()?.fields?.["request.id"], context.requestId);
      throw new Error("private authentication detail");
    }
  });

  const response = await middleware(request("auth-failure"), async () => {
    throw new Error("handler must not run");
  });
  const body = await response.text();

  assert.equal(response.status, 500);
  assert.equal(response.headers.get("x-request-id"), "auth-failure");
  assert.doesNotMatch(body, /private authentication detail/);
  assert.equal(reports.length, 1);
  assert.equal(reports[0].requestId, "auth-failure");
  assert.equal(reports[0].logRequestId, "auth-failure");
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
});

test("post-processing exceptions retain authenticated actor and tenant context", async () => {
  const reports = [];
  const middleware = createMiddleware(testConfig(), {
    operationFailureReporter: recorder(reports),
    authVerifier: async () => ({
      userId: "user-42",
      tenantId: "tenant-7",
      claims: { "otel.plan": "pro", ignored: "secret" }
    }),
    captureSchema: async () => {
      assert.equal(currentContext()?.userId, "user-42");
      assert.equal(currentContext()?.tenantId, "tenant-7");
      assert.equal(getLogContext()?.fields?.["user.id"], "user-42");
      assert.equal(getLogContext()?.fields?.["tenant.id"], "tenant-7");
      assert.equal(getLogContext()?.fields?.["baggage.otel.plan"], "pro");
      assert.equal(getLogContext()?.fields?.["baggage.ignored"], undefined);
      throw new Error("private schema detail");
    }
  });

  const response = await middleware(request("post-failure"), async () => {
    assert.equal(currentContext()?.userId, "user-42");
    assert.equal(getLogContext()?.fields?.["user.id"], "user-42");
    return Response.json({ ok: true });
  });
  const body = await response.text();

  assert.equal(response.status, 500);
  assert.equal(response.headers.get("x-request-id"), "post-failure");
  assert.doesNotMatch(body, /private schema detail/);
  assert.equal(reports.length, 1);
  assert.equal(reports[0].requestId, "post-failure");
  assert.equal(reports[0].userId, "user-42");
  assert.equal(reports[0].logUserId, "user-42");
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
});

test("parallel policy failures never bleed request or log context", async () => {
  const reports = [];
  const middleware = createMiddleware(testConfig(), {
    operationFailureReporter: recorder(reports),
    authVerifier: async (_request, context) => {
      await new Promise((resolve) => setTimeout(resolve, Number(context.requestId.slice(1)) % 4));
      assert.equal(currentContext()?.requestId, context.requestId);
      assert.equal(getLogContext()?.fields?.["request.id"], context.requestId);
      throw new Error("isolated policy failure");
    }
  });

  const responses = await Promise.all(
    Array.from({ length: 32 }, (_, slot) =>
      middleware(request(`r${slot}`), async () => Response.json({ ok: true }))
    )
  );

  assert.ok(responses.every((response) => response.status === 500));
  assert.equal(reports.length, 32);
  for (const report of reports) {
    assert.equal(report.requestId, report.failure.requestId);
    assert.equal(report.logRequestId, report.failure.requestId);
  }
  assert.equal(new Set(reports.map((report) => report.failure.requestId)).size, 32);
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
});

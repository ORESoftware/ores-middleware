import assert from "node:assert/strict";
import test from "node:test";

import {
  bindRequestContext,
  captureRequestContext,
  contextForRequest,
  currentContext,
  getCorrelationId,
  getLoggedInUserId,
  getRequestId,
  getSessionId,
  getTenantId,
  runWithCapturedRequestContext,
  runWithContext
} from "../dist/context.js";
import { getLogContext } from "../dist/otel.js";
import {
  attachFrameworkRequestContext,
  frameworkRequestContext,
  nextjsEdgeHandler,
  nextjsNodeHandler
} from "../dist/adapters.js";

const turn = () => new Promise((resolve) => setImmediate(resolve));

function requestContext(id) {
  return {
    schema: "ores.request-context.v1",
    requestId: `request-${id}`,
    traceId: id.padEnd(32, "0").slice(0, 32),
    spanId: id.padEnd(16, "0").slice(0, 16),
    loggedInUserId: `user-${id}`,
    userId: `user-${id}`,
    tenantId: `tenant-${id}`,
    sessionId: `session-${id}`,
    correlationId: `correlation-${id}`,
    startedAtUnixMs: 1_000,
    deadlineUnixMs: 2_000,
    baggage: { "otel.region": id }
  };
}

test("middleware and ores-otel read one AsyncLocalStorage frame", async () => {
  await Promise.all(
    ["alpha", "beta", "gamma"].map((id) =>
      runWithContext(requestContext(id), async () => {
        await turn();
        assert.equal(getRequestId(), `request-${id}`);
        assert.equal(getLoggedInUserId(), `user-${id}`);
        assert.equal(getTenantId(), `tenant-${id}`);
        assert.equal(getSessionId(), `session-${id}`);
        assert.equal(getCorrelationId(), `correlation-${id}`);
        assert.equal(currentContext().requestId, `request-${id}`);
        assert.equal(getLogContext().fields["request.id"], `request-${id}`);
        assert.equal(getLogContext().fields["user.id"], `user-${id}`);
      })
    )
  );
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
});

test("captured contexts explicitly cross detached task and queue boundaries", async () => {
  const snapshot = await runWithContext(requestContext("capture"), async () => {
    await turn();
    return captureRequestContext();
  });

  assert.equal(getRequestId(), undefined);
  await runWithCapturedRequestContext(snapshot, async () => {
    await turn();
    assert.equal(getRequestId(), "request-capture");
    assert.equal(getLoggedInUserId(), "user-capture");
  });
  assert.equal(getRequestId(), undefined);
});

test("Request-bound fallback is immutable and works without ambient lookup", () => {
  const request = new Request("https://example.test/profile");
  bindRequestContext(request, requestContext("bound"));

  const first = contextForRequest(request);
  assert.equal(first.requestId, "request-bound");
  first.baggage["otel.region"] = "mutated";
  assert.equal(contextForRequest(request).baggage["otel.region"], "bound");
});

test("framework adapters expose the same explicit snapshot", async () => {
  const nativeRequest = {};
  attachFrameworkRequestContext(nativeRequest, requestContext("framework"));
  assert.equal(
    frameworkRequestContext(nativeRequest).loggedInUserId,
    "user-framework"
  );

  const middleware = async (request, next) => {
    const context = requestContext("next");
    bindRequestContext(request, context);
    return runWithContext(context, () => next(request));
  };

  const nodeHandler = nextjsNodeHandler(middleware, async (_request, context) => {
    assert.equal(context.requestId, "request-next");
    assert.equal(getRequestId(), "request-next");
    return new Response("node");
  });
  assert.equal(
    await (await nodeHandler(new Request("https://example.test/node"))).text(),
    "node"
  );

  const edgeHandler = nextjsEdgeHandler(middleware, async (_request, context) => {
    assert.equal(context.requestId, "request-next");
    return new Response("edge");
  });
  assert.equal(
    await (await edgeHandler(new Request("https://example.test/edge"))).text(),
    "edge"
  );
});

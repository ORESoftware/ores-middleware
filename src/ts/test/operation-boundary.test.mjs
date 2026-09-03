import test from "node:test";
import assert from "node:assert/strict";

import {
  bindOperationBoundary,
  captureOperationContext,
  runOperationBoundary,
  runWithCapturedOperationContext
} from "../dist/operation.js";
import {
  currentContext,
  runWithContext
} from "../dist/context.js";
import {
  getLogContext,
  runWithLogContext
} from "@oresoftware/next-loggers/context";

function requestContext(slot) {
  return {
    requestId: `request-${slot}`,
    traceId: slot.toString(16).padStart(32, "0"),
    userId: `user-${slot}`,
    tenantId: `tenant-${slot}`,
    startedAtUnixMs: 0,
    baggage: {}
  };
}

function register(slot, operation, reports) {
  const request = requestContext(slot);
  const log = {
    fields: {
      "request.id": request.requestId,
      "trace.id": request.traceId,
      "user.id": request.userId,
      "tenant.id": request.tenantId
    }
  };
  return runWithContext(request, () =>
    runWithLogContext(log, () =>
      bindOperationBoundary(
        { transport: "websocket", scope: "message", name: "chat.message" },
        operation,
        {
          reportFailure: async ({ failure }) => {
            reports.push({
              failure,
              requestId: currentContext()?.requestId,
              logRequestId: getLogContext()?.fields?.["request.id"]
            });
          }
        }
      )
    )
  );
}

test("a failed WebSocket message is isolated and a later message still runs", async () => {
  const reports = [];
  const failed = register(1, async () => {
    throw new Error("secret payload must not be copied");
  }, reports);
  const succeeded = register(1, async (value) => value.toUpperCase(), reports);

  const first = await failed();
  const second = await succeeded("ok");

  assert.equal(first.ok, false);
  assert.equal(first.failure.requestId, "request-1");
  assert.equal(first.failure.traceId, "00000000000000000000000000000001");
  assert.equal(first.failure.errorType, "Error");
  assert.doesNotMatch(JSON.stringify(first.failure), /secret payload/);
  assert.deepEqual(second, { ok: true, value: "OK" });
  assert.equal(reports.length, 1);
  assert.equal(reports[0].requestId, "request-1");
  assert.equal(reports[0].logRequestId, "request-1");
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
});

test("parallel TCP callbacks retain their own request and trace context", async () => {
  const reports = [];
  const callbacks = Array.from({ length: 48 }, (_, slot) =>
    register(slot, async () => {
      await new Promise((resolve) => setTimeout(resolve, (47 - slot) % 7));
      assert.equal(currentContext()?.requestId, `request-${slot}`);
      assert.equal(getLogContext()?.fields?.["request.id"], `request-${slot}`);
      if (slot % 5 === 0) throw new TypeError("connection frame rejected");
      return slot;
    }, reports)
  );

  const outcomes = await Promise.all(callbacks.map((callback) => callback()));
  assert.equal(outcomes.filter((outcome) => !outcome.ok).length, 10);
  assert.equal(reports.length, 10);
  for (const report of reports) {
    assert.equal(report.requestId, report.failure.requestId);
    assert.equal(report.logRequestId, report.failure.requestId);
  }
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
});

test("a throwing reporter is fail-open and cannot replace the operation outcome", async () => {
  const context = requestContext(77);
  const outcome = await runWithContext(context, () =>
    runOperationBoundary(
      { transport: "http", scope: "request", name: "orders.read" },
      async () => { throw new Error("handler failed"); },
      { reportFailure: () => { throw new Error("collector failed"); } }
    )
  );

  assert.equal(outcome.ok, false);
  assert.equal(outcome.failure.code, "operation_failed");
  assert.equal(currentContext(), undefined);
});

test("an explicitly empty captured frame clears and restores an unrelated caller", async () => {
  const empty = captureOperationContext();
  const outer = requestContext(91);
  const outcome = await runWithContext(outer, () =>
    runWithLogContext({ fields: { "request.id": outer.requestId } }, () =>
      runOperationBoundary(
        { transport: "tcp", scope: "connection", name: "smtp.accept" },
        () => ({
          request: currentContext(),
          log: getLogContext()
        }),
        { context: empty, reportFailure: () => {} }
      )
    )
  );

  assert.equal(outcome.ok, true);
  assert.equal(outcome.value.request, undefined);
  assert.equal(outcome.value.log?.fields?.["request.id"], undefined);
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
});

test("captured scopes can be re-entered without installing an error boundary", () => {
  const request = requestContext(5);
  const snapshot = runWithContext(request, () =>
    runWithLogContext({ fields: { "request.id": request.requestId } }, captureOperationContext)
  );

  const observed = runWithCapturedOperationContext(snapshot, () => ({
    request: currentContext()?.requestId,
    log: getLogContext()?.fields?.["request.id"]
  }));
  assert.deepEqual(observed, { request: "request-5", log: "request-5" });
  assert.equal(currentContext(), undefined);
  assert.equal(getLogContext(), undefined);
});

test("unbounded operation names are normalized before reporting", async () => {
  const reports = [];
  const outcome = await runOperationBoundary(
    { transport: "tcp", scope: "callback", name: "customer/" + "x".repeat(300) },
    () => { throw new Error("must stay private"); },
    { reportFailure: ({ failure }) => { reports.push(failure); } }
  );

  assert.equal(outcome.ok, false);
  assert.equal(outcome.failure.operation, "operation");
  assert.equal(reports[0].operation, "operation");
  assert.doesNotMatch(JSON.stringify(reports[0]), /must stay private/);
});

test("aborted operations are classified inside the captured request scope", async () => {
  const controller = new AbortController();
  controller.abort(new DOMException("private timeout detail", "TimeoutError"));
  const request = requestContext(101);
  const reports = [];
  const outcome = await runWithContext(request, () =>
    runOperationBoundary(
      {
        transport: "http",
        scope: "request",
        name: "orders.read",
        signal: controller.signal
      },
      () => "must not run",
      {
        reportFailure: ({ failure }) => {
          reports.push({ failure, current: currentContext()?.requestId });
        }
      }
    )
  );

  assert.equal(outcome.ok, false);
  assert.equal(outcome.failure.kind, "deadline_exceeded");
  assert.equal(outcome.failure.code, "operation_deadline_exceeded");
  assert.equal(reports[0].current, "request-101");
  assert.doesNotMatch(JSON.stringify(reports[0]), /private timeout detail/);
});

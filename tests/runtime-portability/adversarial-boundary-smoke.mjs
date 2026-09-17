import {
  providerFrom,
  fallibleProviderFrom,
  providerError,
  composeMiddleware,
} from "../../src/ts/dist/generic.js";
import {
  bindContext,
  currentContext,
  runWithCapturedContext,
  runWithContext,
} from "../../src/ts/dist/context.js";

function fail(message) {
  throw new Error(message);
}

function assert(condition, message) {
  if (!condition) fail(message);
}

function assertEqual(actual, expected, message) {
  if (actual !== expected) {
    fail(`${message}: actual=${JSON.stringify(actual)} expected=${JSON.stringify(expected)}`);
  }
}

function runtimeName() {
  if (globalThis.Bun) return `bun-${globalThis.Bun.version}`;
  if (globalThis.Deno) return `deno-${globalThis.Deno.version.deno}`;
  if (globalThis.process?.versions?.node) return `node-${globalThis.process.versions.node}`;
  return "unknown";
}

function context(id) {
  return {
    requestId: `request-${id}`,
    traceId: `trace-${id}`,
    tenantId: `tenant-${id}`,
    userId: `user-${id}`,
    startedAtUnixMs: 1,
    deadlineUnixMs: 2,
    baggage: { marker: id },
  };
}

let checks = 0;

const sentinel = Object.freeze({ kind: "provider-sentinel" });
const throwingProvider = providerFrom(async () => {
  throw sentinel;
});
let observed;
try {
  await throwingProvider.verify("bad");
} catch (error) {
  observed = error;
}
assert(observed === sentinel, "providerFrom must preserve thrown error identity");
checks += 1;

const typedFailure = Object.freeze({ code: "bad_token", retryable: false });
const fallible = fallibleProviderFrom(async () => providerError(typedFailure));
const failed = await fallible.verify("bad");
assert(failed.ok === false, "fallible provider must preserve explicit failure channel");
assert(failed.error === typedFailure, "fallible provider must preserve failure identity");
checks += 1;

const order = [];
const first = (next) => async (request) => {
  order.push("first:before");
  try {
    return await next(request);
  } finally {
    order.push("first:finally");
  }
};
const second = (next) => async (request) => {
  order.push("second:before");
  return next(request);
};
const handlerError = new Error("handler-failure");
const composed = composeMiddleware(
  async () => {
    order.push("handler");
    throw handlerError;
  },
  first,
  second,
);
let composedError;
try {
  await composed("request");
} catch (error) {
  composedError = error;
}
assert(composedError === handlerError, "middleware composition must preserve handler error identity");
assertEqual(order.join(","), "first:before,second:before,handler,first:finally", "failure unwind order");
checks += 1;

assertEqual(currentContext(), undefined, "ambient context must begin empty");
let contextError;
try {
  await runWithContext(context("failure"), async () => {
    assertEqual(currentContext()?.requestId, "request-failure", "context visible before failure");
    await Promise.resolve();
    throw handlerError;
  });
} catch (error) {
  contextError = error;
}
assert(contextError === handlerError, "runWithContext must preserve failure identity");
assertEqual(currentContext(), undefined, "failed context scope must restore empty ambient state");
checks += 1;

await runWithContext(context("outer"), async () => {
  await runWithCapturedContext(undefined, async () => {
    assertEqual(currentContext(), undefined, "undefined snapshot must clear unrelated ambient context");
    await Promise.resolve();
    assertEqual(currentContext(), undefined, "cleared snapshot must stay clear across microtasks");
  });
  assertEqual(currentContext()?.requestId, "request-outer", "outer context restored after explicit clear");
});
assertEqual(currentContext(), undefined, "outer clear must restore process ambient state");
checks += 1;

const boundOutside = bindContext(async () => {
  await Promise.resolve();
  return currentContext()?.requestId;
});
await runWithContext(context("unrelated"), async () => {
  assertEqual(
    await boundOutside(),
    undefined,
    "callback bound outside a request must not inherit later unrelated context",
  );
});
assertEqual(currentContext(), undefined, "bound callback test must not leak context");
checks += 1;

const original = context("snapshot");
await runWithContext(original, async () => {
  original.baggage.marker = "mutated-after-entry";
  assertEqual(
    currentContext()?.baggage.marker,
    "snapshot",
    "runWithContext must isolate ambient baggage from caller mutation",
  );
});
assertEqual(currentContext(), undefined, "snapshot mutation test must clear ambient state");
checks += 1;

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "adversarial-boundary",
  status: "passed",
  runtime: runtimeName(),
  checks,
}));

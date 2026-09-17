import {
  providerFrom,
  fallibleProviderFrom,
  providerError,
  composeMiddleware,
} from "../../target/ts/dist/generic.js";
import {
  bindContext,
  currentContext,
  runWithCapturedContext,
  runWithContext,
} from "../../target/ts/dist/context.js";

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

const sentinel = Object.freeze({ kind: "packaged-provider-sentinel" });
const throwingProvider = providerFrom(async () => {
  throw sentinel;
});
let observed;
try {
  await throwingProvider.verify("bad");
} catch (error) {
  observed = error;
}
assert(observed === sentinel, "packaged provider must preserve thrown identity");
checks += 1;

const typedFailure = Object.freeze({ code: "bad_token", retryable: false });
const fallible = fallibleProviderFrom(async () => providerError(typedFailure));
const failed = await fallible.verify("bad");
assert(failed.ok === false, "packaged fallible provider must preserve failure channel");
assert(failed.error === typedFailure, "packaged fallible provider must preserve failure identity");
checks += 1;

const order = [];
const outer = (next) => async (request) => {
  order.push("outer:before");
  try {
    return await next(request);
  } finally {
    order.push("outer:finally");
  }
};
const inner = (next) => async (request) => {
  order.push("inner:before");
  return next(request);
};
const handlerError = new Error("packaged-handler-failure");
const composed = composeMiddleware(
  async () => {
    order.push("handler");
    throw handlerError;
  },
  outer,
  inner,
);
let composedError;
try {
  await composed("request");
} catch (error) {
  composedError = error;
}
assert(composedError === handlerError, "packaged composition must preserve handler error identity");
assertEqual(order.join(","), "outer:before,inner:before,handler,outer:finally", "packaged unwind order");
checks += 1;

assertEqual(currentContext(), undefined, "packaged ambient context must begin empty");
let scopeError;
try {
  await runWithContext(context("failure"), async () => {
    assertEqual(currentContext()?.requestId, "request-failure", "packaged context before failure");
    throw handlerError;
  });
} catch (error) {
  scopeError = error;
}
assert(scopeError === handlerError, "packaged context scope must preserve thrown error");
assertEqual(currentContext(), undefined, "packaged failed scope must clean ambient context");
checks += 1;

await runWithContext(context("outer"), async () => {
  await runWithCapturedContext(undefined, async () => {
    assertEqual(currentContext(), undefined, "packaged absent snapshot clears unrelated context");
  });
  assertEqual(currentContext()?.requestId, "request-outer", "packaged outer context restored");
});
assertEqual(currentContext(), undefined, "packaged outer scope must clear");
checks += 1;

const boundOutside = bindContext(async () => currentContext()?.requestId);
await runWithContext(context("later"), async () => {
  assertEqual(await boundOutside(), undefined, "packaged callback bound outside must not inherit later scope");
});
assertEqual(currentContext(), undefined, "packaged bound callback must not leak");
checks += 1;

const original = context("snapshot");
await runWithContext(original, async () => {
  original.baggage.marker = "mutated";
  assertEqual(currentContext()?.baggage.marker, "snapshot", "packaged context must snapshot baggage");
});
assertEqual(currentContext(), undefined, "packaged snapshot test must clear");
checks += 1;

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "packaged-adversarial-boundary",
  status: "passed",
  runtime: runtimeName(),
  checks,
}));

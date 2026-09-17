import {
  composeContextualMiddleware,
  composeMiddleware,
  composeNamedContextualMiddleware,
  composeNamedMiddleware,
  contextualFallibleProviderFrom,
  contextualProviderFrom,
  fallibleProviderFrom,
  providerError,
  providerFrom,
  providerOk,
} from "../../target/ts/dist/generic.js";
import {
  bindContext,
  captureContext,
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

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, canonical(value[key])]),
    );
  }
  return value;
}

function assertDeepEqual(actual, expected, message) {
  const left = JSON.stringify(canonical(actual));
  const right = JSON.stringify(canonical(expected));
  if (left !== right) fail(`${message}: actual=${left} expected=${right}`);
}

function runtimeName() {
  if (globalThis.Bun) return `bun-${globalThis.Bun.version}`;
  if (globalThis.Deno) return `deno-${globalThis.Deno.version.deno}`;
  if (globalThis.process?.versions?.node) return `node-${globalThis.process.versions.node}`;
  return "unknown";
}

function requestContext(id) {
  return Object.freeze({
    requestId: `request-${id}`,
    traceId: `trace-${id}`,
    tenantId: `tenant-${id}`,
    userId: `user-${id}`,
    startedAtUnixMs: 1,
    deadlineUnixMs: 2,
    baggage: Object.freeze({ marker: id }),
  });
}

let checks = 0;

const provider = providerFrom(async (token) => ({ subject: token }));
assertDeepEqual(await provider.verify("alice"), { subject: "alice" }, "packaged provider");
checks += 1;

const fallible = fallibleProviderFrom(async (token) =>
  token === "ok" ? providerOk("accepted") : providerError("rejected"),
);
assertDeepEqual(await fallible.verify("ok"), { ok: true, value: "accepted" }, "packaged result success");
assertDeepEqual(await fallible.verify("bad"), { ok: false, error: "rejected" }, "packaged result failure");
checks += 1;

const contextual = contextualProviderFrom(async (request, context) => `${context.scope}:${request}`);
const contextualFallible = contextualFallibleProviderFrom(async (request, context) =>
  request === "ok" ? providerOk(context.scope) : providerError("rejected"),
);
assertEqual(
  await contextual.verify({ request: "req", context: { scope: "tenant" } }),
  "tenant:req",
  "packaged contextual provider",
);
assertDeepEqual(
  await contextualFallible.verify({ request: "bad", context: { scope: "tenant" } }),
  { ok: false, error: "rejected" },
  "packaged contextual failure",
);
checks += 1;

const events = [];
const stage = (name) => (next) => async (request) => {
  events.push(`${name}:before`);
  const response = await next(request);
  events.push(`${name}:after`);
  return response;
};
const composed = composeMiddleware(
  async (request) => `${request}:ok`,
  stage("outer"),
  stage("inner"),
);
assertEqual(await composed("request"), "request:ok", "packaged composed response");
assertDeepEqual(
  events,
  ["outer:before", "inner:before", "inner:after", "outer:after"],
  "packaged middleware order",
);
checks += 1;

const namedEvents = [];
const namedStage = (name) => ({
  name,
  middleware: (next) => async (request) => {
    namedEvents.push(name);
    return next(request);
  },
});
const named = composeNamedMiddleware(
  async (request) => request,
  [namedStage("dup"), namedStage("dup"), namedStage("tail")],
);
assertEqual(await named("request"), "request", "packaged named response");
assertDeepEqual(namedEvents, ["dup", "dup", "tail"], "packaged duplicate preservation");
checks += 1;

const contextualEvents = [];
const contextualStage = (name) => ({
  name,
  middleware: (next) => async (request, context) => {
    contextualEvents.push(`${name}:${context.scope}`);
    return next(request, context);
  },
});
const namedContextual = composeNamedContextualMiddleware(
  async (request, context) => `${context.scope}:${request}`,
  [contextualStage("z"), contextualStage("a")],
);
assertEqual(
  await namedContextual("request", { scope: "scope" }),
  "scope:request",
  "packaged named contextual response",
);
assertDeepEqual(contextualEvents, ["z:scope", "a:scope"], "packaged named contextual order");
checks += 1;

const identity = async (request) => request;
const contextualIdentity = async (request, context) => ({ request, context });
assert(composeMiddleware(identity) === identity, "packaged empty middleware identity");
assert(composeNamedMiddleware(identity, []) === identity, "packaged empty named identity");
assert(
  composeContextualMiddleware(contextualIdentity) === contextualIdentity,
  "packaged empty contextual identity",
);
assert(
  composeNamedContextualMiddleware(contextualIdentity, []) === contextualIdentity,
  "packaged empty named contextual identity",
);
checks += 1;

assertEqual(currentContext(), undefined, "packaged ambient context must begin empty");
await runWithContext(requestContext("outer"), async () => {
  assertEqual(currentContext()?.requestId, "request-outer", "packaged outer context");
  const captured = captureContext();
  await runWithContext(requestContext("inner"), async () => {
    await Promise.resolve();
    assertEqual(currentContext()?.requestId, "request-inner", "packaged inner async context");
  });
  assertEqual(currentContext()?.requestId, "request-outer", "packaged outer restore");
  await runWithCapturedContext(captured, async () => {
    assertEqual(currentContext()?.requestId, "request-outer", "packaged captured reentry");
  });
});
assertEqual(currentContext(), undefined, "packaged context must clear");
checks += 1;

const isolated = await Promise.all(
  Array.from({ length: 32 }, (_, index) =>
    runWithContext(requestContext(String(index)), async () => {
      const bound = bindContext(async () => {
        await new Promise((resolve) => setTimeout(resolve, index % 3));
        return currentContext()?.requestId;
      });
      const value = await bound();
      assertEqual(value, `request-${index}`, `packaged isolation ${index}`);
      return value;
    }),
  ),
);
assertEqual(new Set(isolated).size, 32, "packaged parallel contexts must stay isolated");
assertEqual(currentContext(), undefined, "packaged parallel contexts must not leak");
checks += 1;

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "packaged-generic-context",
  status: "passed",
  runtime: runtimeName(),
  checks,
  concurrentContexts: isolated.length,
}));

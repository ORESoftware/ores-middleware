import { descriptor } from "../../src/ts/dist/index.js";
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
} from "../../src/ts/dist/generic.js";
import {
  bindContext,
  captureContext,
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

const runtimeDescriptor = descriptor();
assertEqual(runtimeDescriptor.language, "ts", "descriptor language");
assertEqual(runtimeDescriptor.runtime, "node-deno-bun", "descriptor runtime claim");
assert(runtimeDescriptor.frameworkAdapters.includes("bun"), "descriptor must declare Bun adapter");
assert(runtimeDescriptor.frameworkAdapters.includes("deno"), "descriptor must declare Deno adapter");
checks += 1;

const sdk = Object.freeze({ version: "v17", prefix: "v17:" });
const provider = providerFrom(async (token) => {
  assertEqual(sdk.version, "v17", "consumer SDK identity must stay captured");
  if (!token.startsWith(sdk.prefix)) throw new Error("rejected");
  return { subject: token.slice(sdk.prefix.length) };
});
assertDeepEqual(
  await provider.verify("v17:alice"),
  { subject: "alice" },
  "provider output",
);
checks += 1;

const fallible = fallibleProviderFrom(async (token) =>
  token === "ok"
    ? providerOk({ subject: "accepted" })
    : providerError({ code: "bad_token", retryable: false }),
);
assertDeepEqual(
  await fallible.verify("ok"),
  { ok: true, value: { subject: "accepted" } },
  "typed provider success",
);
assertDeepEqual(
  await fallible.verify("bad"),
  { ok: false, error: { code: "bad_token", retryable: false } },
  "typed provider failure",
);
checks += 1;

const sentinel = new Error("consumer-owned-provider-error");
const throwingProvider = providerFrom(async () => {
  throw sentinel;
});
let observed;
try {
  await throwingProvider.verify("request");
} catch (error) {
  observed = error;
}
assert(observed === sentinel, "provider exception identity must remain consumer-owned");
checks += 1;

const contextual = contextualProviderFrom(
  async (request, context) => `${context.tenant}:${request.token}`,
);
const contextualFallible = contextualFallibleProviderFrom(
  async (request, context) =>
    request.token === "ok"
      ? providerOk({ tenant: context.tenant })
      : providerError("rejected"),
);
assertEqual(
  await contextual.verify({ request: { token: "abc" }, context: { tenant: "t-1" } }),
  "t-1:abc",
  "contextual provider",
);
assertDeepEqual(
  await contextualFallible.verify({ request: { token: "bad" }, context: { tenant: "t-2" } }),
  { ok: false, error: "rejected" },
  "contextual fallible provider",
);
checks += 1;

const events = [];
const stage = (name) => (next) => async (request) => {
  events.push(`${name}:before`);
  const response = await next(request);
  events.push(`${name}:after`);
  return response;
};
const baseHandler = async (request) => {
  events.push("handler");
  return `${request}:ok`;
};
const composed = composeMiddleware(
  baseHandler,
  stage("request-id"),
  stage("consumer-auth-v17"),
  stage("tenant-rate-limit"),
);
assertEqual(await composed("request"), "request:ok", "composed response");
assertDeepEqual(
  events,
  [
    "request-id:before",
    "consumer-auth-v17:before",
    "tenant-rate-limit:before",
    "handler",
    "tenant-rate-limit:after",
    "consumer-auth-v17:after",
    "request-id:after",
  ],
  "declaration order must be preserved",
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
  [namedStage("auth"), namedStage("auth"), namedStage("audit")],
);
assertEqual(await named("request"), "request", "named middleware response");
assertDeepEqual(namedEvents, ["auth", "auth", "audit"], "duplicate names must be preserved");
checks += 1;

const contextualEvents = [];
const contextualStage = (name) => ({
  name,
  middleware: (next) => async (request, context) => {
    contextualEvents.push(`${name}:${context.tenant}`);
    return next(request, context);
  },
});
const contextualHandler = async (request, context) => `${context.tenant}:${request}`;
const namedContextual = composeNamedContextualMiddleware(
  contextualHandler,
  [contextualStage("z-stage"), contextualStage("auth-provider-v42"), contextualStage("a-stage")],
);
assertEqual(
  await namedContextual("request", { tenant: "t-99" }),
  "t-99:request",
  "named contextual middleware response",
);
assertDeepEqual(
  contextualEvents,
  ["z-stage:t-99", "auth-provider-v42:t-99", "a-stage:t-99"],
  "named contextual order must stay opaque",
);
checks += 1;

const emptyHandler = async (request) => request;
const emptyContextualHandler = async (request, context) => ({ request, context });
assert(composeMiddleware(emptyHandler) === emptyHandler, "empty middleware chain must preserve handler identity");
assert(composeNamedMiddleware(emptyHandler, []) === emptyHandler, "empty named chain must preserve handler identity");
assert(
  composeContextualMiddleware(emptyContextualHandler) === emptyContextualHandler,
  "empty contextual chain must preserve handler identity",
);
assert(
  composeNamedContextualMiddleware(emptyContextualHandler, []) === emptyContextualHandler,
  "empty named contextual chain must preserve handler identity",
);
checks += 1;

assertEqual(currentContext(), undefined, "ambient context must start empty");
const outer = requestContext("outer");
const inner = requestContext("inner");
await runWithContext(outer, async () => {
  assertEqual(currentContext()?.requestId, "request-outer", "outer context visible");
  const snapshot = captureContext();
  assert(snapshot !== outer, "captured context must be a defensive snapshot");
  await runWithContext(inner, async () => {
    assertEqual(currentContext()?.requestId, "request-inner", "inner context visible");
    await Promise.resolve();
    assertEqual(currentContext()?.requestId, "request-inner", "inner context survives microtasks");
  });
  assertEqual(currentContext()?.requestId, "request-outer", "outer context restored");
  await runWithCapturedContext(snapshot, async () => {
    assertEqual(currentContext()?.requestId, "request-outer", "captured context can be re-entered");
  });
});
assertEqual(currentContext(), undefined, "ambient context must be cleared after scope");
checks += 1;

const isolation = await Promise.all(
  Array.from({ length: 32 }, (_, index) =>
    runWithContext(requestContext(String(index)), async () => {
      const bound = bindContext(async () => {
        await new Promise((resolve) => setTimeout(resolve, index % 4));
        return currentContext()?.requestId;
      });
      await Promise.resolve();
      const value = await bound();
      assertEqual(value, `request-${index}`, `parallel context isolation ${index}`);
      return value;
    }),
  ),
);
assertEqual(new Set(isolation).size, 32, "parallel scopes must remain isolated");
assertEqual(currentContext(), undefined, "parallel scopes must not leak ambient context");
checks += 1;

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "generic-context",
  status: "passed",
  runtime: runtimeName(),
  checks,
  concurrentContexts: isolation.length,
}));

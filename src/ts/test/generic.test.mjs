import assert from "node:assert/strict";
import test from "node:test";

import {
  composeContextualMiddleware,
  composeMiddleware,
  contextualFallibleProviderFrom,
  contextualProviderFrom,
  fallibleProviderFrom,
  providerError,
  providerFrom,
  providerOk
} from "../dist/generic.js";

test("generic provider keeps concrete SDK ownership in the consumer", async () => {
  const sdk = Object.freeze({ version: "v7", prefix: "sdk-v7:" });
  const provider = providerFrom(async (token) => {
    assert.equal(sdk.version, "v7");
    if (!token.startsWith(sdk.prefix)) throw new Error("rejected");
    return { subject: token.slice(sdk.prefix.length) };
  });

  assert.deepEqual(await provider.verify("sdk-v7:alice"), { subject: "alice" });
});

test("typed provider failures remain consumer-defined and SDK agnostic", async () => {
  const sdk = Object.freeze({ prefix: "v9:" });
  const provider = fallibleProviderFrom(async (token) =>
    token.startsWith(sdk.prefix)
      ? providerOk({ subject: token.slice(sdk.prefix.length) })
      : providerError({ code: "bad_token", retryable: false })
  );

  assert.deepEqual(await provider.verify("v9:bob"), {
    ok: true,
    value: { subject: "bob" }
  });
  assert.deepEqual(await provider.verify("wrong"), {
    ok: false,
    error: { code: "bad_token", retryable: false }
  });
});

test("contextual providers are generic over request, context, output, and failure", async () => {
  const provider = contextualProviderFrom(
    async (request, context) => `${context.tenant}:${request.token}`
  );
  const fallible = contextualFallibleProviderFrom(
    async (request, context) => request.token === "ok"
      ? providerOk({ tenant: context.tenant, accepted: true })
      : providerError("rejected")
  );

  assert.equal(
    await provider.verify({ request: { token: "abc" }, context: { tenant: "t-1" } }),
    "t-1:abc"
  );
  assert.deepEqual(
    await fallible.verify({ request: { token: "ok" }, context: { tenant: "t-2" } }),
    { ok: true, value: { tenant: "t-2", accepted: true } }
  );
});

test("middleware order is selected entirely by the consumer", async () => {
  const events = [];
  const stage = (name) => (next) => async (request) => {
    events.push(`${name}:before`);
    const response = await next(request);
    events.push(`${name}:after`);
    return response;
  };

  const handler = async (request) => {
    events.push("handler");
    return `${request}:ok`;
  };

  const composed = composeMiddleware(
    handler,
    stage("request-id"),
    stage("consumer-auth-v7"),
    stage("tenant-rate-limit")
  );

  assert.equal(await composed("request"), "request:ok");
  assert.deepEqual(events, [
    "request-id:before",
    "consumer-auth-v7:before",
    "tenant-rate-limit:before",
    "handler",
    "tenant-rate-limit:after",
    "consumer-auth-v7:after",
    "request-id:after"
  ]);
});

test("contextual middleware preserves consumer-owned context shape and order", async () => {
  const events = [];
  const stage = (name) => (next) => async (request, context) => {
    events.push(`${name}:${context.scope}:before`);
    const response = await next(request, context);
    events.push(`${name}:${context.scope}:after`);
    return response;
  };
  const handler = async (request, context) => `${context.scope}:${request}`;
  const composed = composeContextualMiddleware(
    handler,
    stage("auth"),
    stage("tenant")
  );

  assert.equal(await composed("request", { scope: "customer" }), "customer:request");
  assert.deepEqual(events, [
    "auth:customer:before",
    "tenant:customer:before",
    "tenant:customer:after",
    "auth:customer:after"
  ]);
});

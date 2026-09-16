import assert from "node:assert/strict";
import test from "node:test";

import {
  composeMiddleware,
  contextualProviderFrom,
  providerFrom
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

test("contextual provider is generic over request and context types", async () => {
  const provider = contextualProviderFrom(
    async (request, context) => `${context.tenant}:${request.token}`
  );

  assert.equal(
    await provider.verify({ request: { token: "abc" }, context: { tenant: "t-1" } }),
    "t-1:abc"
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

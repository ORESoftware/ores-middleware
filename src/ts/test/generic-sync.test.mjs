import assert from "node:assert/strict";
import test from "node:test";

import {
  composeContextualMiddleware,
  composeMiddleware,
  contextualProviderFrom,
  providerFrom,
} from "../dist/generic.js";

test("generic providers accept synchronous consumer implementations", async () => {
  const provider = providerFrom((value) => value.toUpperCase());
  const contextual = contextualProviderFrom((request, context) => `${context}:${request}`);

  assert.equal(await provider.verify("alice"), "ALICE");
  assert.equal(
    await contextual.verify({ request: "request", context: "tenant-a" }),
    "tenant-a:request",
  );
});

test("generic middleware composes synchronous and asynchronous stages", async () => {
  const events = [];
  const syncStage = (next) => (request) => {
    events.push("sync:before");
    const response = next(request);
    events.push("sync:after-call");
    return response;
  };
  const asyncStage = (next) => async (request) => {
    events.push("async:before");
    const response = await next(request);
    events.push("async:after");
    return response;
  };
  const handler = composeMiddleware(
    (request) => `${request}:ok`,
    syncStage,
    asyncStage,
  );

  assert.equal(await handler("request"), "request:ok");
  assert.deepEqual(events, [
    "sync:before",
    "async:before",
    "sync:after-call",
    "async:after",
  ]);
});

test("contextual middleware can remain fully synchronous", async () => {
  const stage = (next) => (request, context) =>
    `${context.prefix}>${next(request, context)}`;
  const handler = composeContextualMiddleware(
    (request, context) => `${context.tenant}:${request}`,
    stage,
  );

  assert.equal(
    await handler("request", { prefix: "outer", tenant: "t-7" }),
    "outer>t-7:request",
  );
});

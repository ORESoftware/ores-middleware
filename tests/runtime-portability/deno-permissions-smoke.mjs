import {
  composeMiddleware,
  providerFrom,
} from "../../src/ts/dist/generic.js";
import {
  currentContext,
  runWithContext,
} from "../../src/ts/dist/context.js";

function fail(message) {
  throw new Error(message);
}

if (!globalThis.Deno) {
  console.log(JSON.stringify({
    schema: "ores.middleware.js-runtime-portability-smoke/v1",
    suite: "deno-permissions",
    status: "skipped",
    reason: "not-deno",
  }));
} else {
  const provider = providerFrom(async (value) => value.toUpperCase());
  if (await provider.verify("ok") !== "OK") fail("provider failed under minimal Deno permissions");

  const handler = composeMiddleware(
    async (value) => value,
    (next) => async (value) => next(`${value}:mw`),
  );
  if (await handler("request") !== "request:mw") fail("middleware composition failed under minimal Deno permissions");

  await runWithContext({
    requestId: "request-minimal-deno",
    traceId: "trace-minimal-deno",
    startedAtUnixMs: 1,
    baggage: {},
  }, async () => {
    await Promise.resolve();
    if (currentContext()?.requestId !== "request-minimal-deno") {
      fail("context propagation failed under minimal Deno permissions");
    }
  });
  if (currentContext() !== undefined) fail("context leaked under minimal Deno permissions");

  console.log(JSON.stringify({
    schema: "ores.middleware.js-runtime-portability-smoke/v1",
    suite: "deno-permissions",
    status: "passed",
    permissions: "none",
  }));
}
